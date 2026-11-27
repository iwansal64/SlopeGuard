//! Read-only operator dashboard for SlopeGuard.
//!
//! Separate crate on purpose: the telemetry path in `rust/slopeguard` is owned
//! by another workstream, so this binary only *reads* the same SQLite file and
//! pushes demo fixtures through the real HTTP receiver.
//!
//! ```text
//! slopeguard-dashboard --addr 127.0.0.1:8090 --db data/raw/slopeguard.db \
//!   --web web --receiver http://127.0.0.1:8080 --tokens config/nodes.tokens
//! GET  /                 static UI
//! GET  /api/data?window_s=900   status + series in one round trip
//! POST /api/replay       push a whitelisted fixture through the receiver
//! ```

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use clap::Parser;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use slopeguard::ingest::{Store, latest_health};

const MAX_HEAD: usize = 8 * 1024;
const MAX_BODY: usize = 4 * 1024;
const MAX_CONN: usize = 24;
const ACCEPT_POLL: Duration = Duration::from_millis(1);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const REF_MAX_AGE_S: i64 = 60;
const STALE_AFTER_S: i64 = 30;
const MAX_POINTS: usize = 600;

#[derive(Parser)]
#[command(
    name = "slopeguard-dashboard",
    about = "Read-only SlopeGuard dashboard"
)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:8090")]
    addr: String,
    #[arg(long, default_value = "data/raw/slopeguard.db")]
    db: String,
    #[arg(long, default_value = "web")]
    web: String,
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    receiver: String,
    #[arg(long, default_value = "config/nodes.tokens")]
    tokens: String,
}

struct App {
    db: String,
    web: String,
    receiver: String,
    tokens: String,
    replay_gate: Mutex<bool>,
}

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn open_store(db: &str) -> Result<Store, String> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("{db}: {e}"))?;
    Ok(Store { conn })
}

#[derive(Debug, Clone)]
struct P {
    node: String,
    role: String,
    ts: i64,
    recv: i64,
    seq: i64,
    status: String,
    batt: Option<f64>,
    rssi: Option<f64>,
    pitch: Option<f64>,
    roll: Option<f64>,
    rain: Option<f64>,
    soil: Option<f64>,
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn epoch(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp())
}

fn load_packets(store: &Store, since: i64) -> Vec<P> {
    let Ok(mut stmt) = store.conn.prepare(
        "SELECT node_id, node_role, ts_device, received_at, seq, sensor_status,
                battery_v, signal_dbm, payload
         FROM (SELECT * FROM raw_packet ORDER BY id DESC LIMIT 6000)
         ORDER BY id",
    ) else {
        return Vec::new();
    };
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<f64>>(6)?,
            r.get::<_, Option<f64>>(7)?,
            r.get::<_, String>(8)?,
        ))
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows.flatten() {
        let (node, role, ts_s, recv_s, seq, status, batt, rssi, payload) = row;
        let (Some(ts), Some(recv)) = (epoch(&ts_s), epoch(&recv_s)) else {
            continue;
        };
        if recv < since {
            continue;
        }
        let Ok(p) = serde_json::from_str::<Value>(&payload) else {
            continue;
        };
        out.push(P {
            node,
            role,
            ts,
            recv,
            seq,
            status,
            batt,
            rssi,
            pitch: num(&p, "tilt_pitch_deg"),
            roll: num(&p, "tilt_roll_deg"),
            rain: num(&p, "rain_mm_interval"),
            soil: num(&p, "soil_moisture_pct"),
        });
    }
    out.sort_by_key(|p| p.ts);
    out
}

/// Causal as-of join: a movement packet may only use a reference packet that is
/// at or before it and no older than `max_age`. Missing stays `None`.
fn as_of_relative(
    movement: &[P],
    reference: &[P],
    max_age: i64,
) -> Vec<(i64, Option<f64>, Option<f64>)> {
    let mut out = Vec::with_capacity(movement.len());
    let mut j = 0usize;
    for m in movement {
        while j < reference.len() && reference[j].ts <= m.ts {
            j += 1;
        }
        let r = j.checked_sub(1).map(|i| &reference[i]);
        let fresh = r.filter(|r| m.ts - r.ts <= max_age);
        let (rp, rr) = match (fresh, m.pitch, m.roll) {
            (Some(r), Some(p), Some(ro)) => {
                (r.pitch.map(|v| r3(p - v)), r.roll.map(|v| r3(ro - v)))
            }
            _ => (None, None),
        };
        out.push((m.ts, rp, rr));
    }
    out
}

struct Inputs {
    rel: Option<f64>,
    tilt_rate: Option<f64>,
    sustained: u32,
    rain15: f64,
    soil: Option<f64>,
    soil_delta: Option<f64>,
    quality: &'static str,
}

// Thresholds mirror `analysis::BaselineConfig::default()` (rain 5 mm/15 min,
// soil 75 %, tilt rate 0.2 deg/min, relative pitch 2.0 deg). This is the
// transparent baseline rule, not a model score.
fn decide(i: &Inputs) -> (&'static str, Vec<&'static str>) {
    let mut reasons = Vec::new();
    if i.quality == "MISSING" {
        return ("UNKNOWN", vec!["NO_DATA"]);
    }
    if i.quality == "DEGRADED" {
        return ("UNKNOWN", vec!["REFERENCE_STALE"]);
    }
    let rel = i.rel.unwrap_or(0.0).abs();
    let rate = i.tilt_rate.unwrap_or(0.0);
    let rel_hit = rel >= 2.0 && i.sustained >= 3;
    if rel_hit {
        reasons.push("REL_TILT_ABOVE_BASELINE");
    }
    if rate >= 0.2 {
        reasons.push("TILT_RATE_RISING");
    }
    if i.rain15 >= 5.0 {
        reasons.push("RAINFALL_LOADING");
    }
    if i.soil.unwrap_or(0.0) >= 75.0 || i.soil_delta.unwrap_or(0.0) >= 2.0 {
        reasons.push("SOIL_MOISTURE_RISING");
    }
    if rel_hit && rate >= 0.2 {
        return ("MOVEMENT_RISK", reasons);
    }
    if reasons.is_empty() {
        ("STABLE", vec!["NO_ABNORMAL_SIGNAL"])
    } else {
        ("WATCH", reasons)
    }
}

fn downsample(mut points: Vec<Value>) -> Vec<Value> {
    if points.len() <= MAX_POINTS {
        return points;
    }
    let last = points.pop().unwrap();
    let bucket = points.len() as f64 / (MAX_POINTS - 1) as f64;
    let mut out = Vec::with_capacity(MAX_POINTS);
    let mut i = 0usize;
    while i < points.len() {
        let end = (((i as f64 + bucket).ceil()) as usize).clamp(i + 1, points.len());
        let slice = &points[i..end];
        let pick = slice
            .iter()
            .enumerate()
            .max_by(|a, b| {
                let va = a.1[3].as_f64().unwrap_or(0.0).abs();
                let vb = b.1[3].as_f64().unwrap_or(0.0).abs();
                va.total_cmp(&vb)
            })
            .map(|(k, _)| k)
            .unwrap_or(0);
        out.push(slice[pick].clone());
        i = end;
    }
    out.push(last);
    out
}

fn point(p: &P, rel: Option<f64>, rel_roll: Option<f64>) -> Value {
    json!([
        p.ts,
        p.pitch.map(r3),
        p.roll.map(r3),
        rel,
        rel_roll,
        p.rain.map(r3),
        p.soil.map(r3),
        p.batt.map(r3),
        p.rssi.map(r3),
        p.seq,
        p.status
    ])
}

fn receiver_addr(url: &str) -> Option<String> {
    let host = url
        .strip_prefix("http://")
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_string();
    let ip = host.split(':').next()?;
    if !matches!(ip, "127.0.0.1" | "localhost" | "[::1]" | "::1") {
        return None;
    }
    Some(host)
}

fn receiver_up(host: &str) -> bool {
    host.parse::<std::net::SocketAddr>()
        .ok()
        .and_then(|a| TcpStream::connect_timeout(&a, Duration::from_millis(150)).ok())
        .is_some()
}

impl App {
    fn data(&self, window_s: i64) -> Value {
        let now = Utc::now().timestamp();
        let since = now - window_s;
        let mut out = json!({
            "now": DateTime::<Utc>::from_timestamp(now, 0).map(|d| d.to_rfc3339()),
            "window_s": window_s,
            "rule": {
                "basis": "evidence-rule",
                "horizon_minutes": 30,
                "thresholds": {
                    "rel_warn_deg": 2.0,
                    "tilt_rate_warn_deg_min": 0.2,
                    "rain_warn_mm": 5.0,
                    "soil_warn_pct": 75.0,
                    "soil_rise_pct": 2.0,
                    "stale_after_s": STALE_AFTER_S
                }
            },
            "model": {"available": false, "report_present": Path::new("data/validation/model_report.txt").exists()},
            "receiver": {"url": self.receiver, "up": receiver_addr(&self.receiver).map(|h| receiver_up(&h)).unwrap_or(false)},
        });

        let store = match open_store(&self.db) {
            Ok(s) => s,
            Err(e) => {
                out["db"] = json!({"ok": false, "path": self.db, "error": e});
                out["rule"]["state"] = json!("UNKNOWN");
                out["rule"]["reasons"] = json!(["DB_UNAVAILABLE"]);
                out["rule"]["quality"] = json!("MISSING");
                out["nodes"] = json!([]);
                out["series"] = json!({"movement": null, "reference": null});
                return out;
            }
        };

        let packets = load_packets(&store, since);
        let movement: Vec<P> = packets
            .iter()
            .filter(|p| p.role == "movement")
            .cloned()
            .collect();
        let reference: Vec<P> = packets
            .iter()
            .filter(|p| p.role == "reference")
            .cloned()
            .collect();

        let rel = as_of_relative(&movement, &reference, REF_MAX_AGE_S);
        let movement_points: Vec<Value> = movement
            .iter()
            .zip(rel.iter())
            .map(|(p, (_, rp, rr))| point(p, *rp, *rr))
            .collect();
        let reference_points: Vec<Value> = reference.iter().map(|p| point(p, None, None)).collect();

        let counts: HashMap<&str, i64> = packets.iter().fold(HashMap::new(), |mut m, p| {
            *m.entry(p.node.as_str()).or_insert(0) += 1;
            m
        });

        let health = latest_health(&store).unwrap_or_default();
        let nodes: Vec<Value> = health
            .iter()
            .map(|h| {
                let age = epoch(&h.received_at).map(|t| now - t);
                json!({
                    "site_id": h.site_id,
                    "node_id": h.node_id,
                    "node_role": h.node_role,
                    "seq": h.seq,
                    "ts_device": h.ts_device,
                    "received_at": h.received_at,
                    "age_s": age,
                    "sensor_status": h.sensor_status,
                    "battery_v": h.battery_v.map(r3),
                    "signal_dbm": h.signal_dbm,
                    "packets": counts.get(h.node_id.as_str()).copied().unwrap_or(0)
                })
            })
            .collect();

        let packets_total: i64 = store
            .conn
            .query_row("SELECT count(*) FROM raw_packet", [], |r| r.get(0))
            .unwrap_or(0);
        let rejects: i64 = store
            .conn
            .query_row("SELECT count(*) FROM rejected", [], |r| r.get(0))
            .unwrap_or(0);
        let last_reject: Option<(String, String)> = store
            .conn
            .query_row(
                "SELECT received_at, reason FROM rejected ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        out["db"] = json!({
            "ok": true,
            "path": self.db,
            "bytes": std::fs::metadata(&self.db).map(|m| m.len()).unwrap_or(0),
            "packets": packets_total,
            "rejects": rejects,
            "last_reject": last_reject.map(|(at, reason)| json!({"at": at, "reason": reason}))
        });
        out["nodes"] = json!(nodes);

        // Rule inputs from the newest movement packet in the window.
        let mv_latest = movement.last();
        let ref_latest = reference.last();
        let mv_fresh = mv_latest
            .map(|p| now - p.recv <= STALE_AFTER_S)
            .unwrap_or(false);
        let ref_fresh = ref_latest
            .map(|p| now - p.recv <= STALE_AFTER_S)
            .unwrap_or(false);
        let mv_ok = mv_latest.map(|p| p.status == "OK").unwrap_or(false);
        let ref_ok = ref_latest.map(|p| p.status == "OK").unwrap_or(false);
        let quality = if mv_latest.is_none() || ref_latest.is_none() {
            "MISSING"
        } else if !(mv_fresh && ref_fresh && mv_ok && ref_ok) {
            "DEGRADED"
        } else {
            "GOOD"
        };
        let (rel_now, tilt_rate, sustained, rain15, soil, soil_delta) = match mv_latest {
            Some(m) => {
                let rel_now = rel.last().and_then(|(_, rp, _)| *rp);
                let five = rel
                    .iter()
                    .rev()
                    .find(|(t, _, _)| *t <= m.ts - 300)
                    .and_then(|(_, rp, _)| *rp);
                let rate = match (rel_now, five) {
                    (Some(a), Some(b)) => Some(r3((a - b) / 5.0)),
                    _ => None,
                };
                let sustained = rel
                    .iter()
                    .rev()
                    .take_while(|(_, rp, _)| rp.map(|v| v.abs() >= 2.0).unwrap_or(false))
                    .count() as u32;
                let rain15: f64 = movement
                    .iter()
                    .filter(|p| p.ts > m.ts - 900)
                    .filter_map(|p| p.rain)
                    .sum();
                let soil_now = movement
                    .iter()
                    .rev()
                    .find(|p| p.soil.is_some())
                    .and_then(|p| p.soil);
                let soil_five = movement
                    .iter()
                    .rev()
                    .find(|p| p.ts <= m.ts - 300 && p.soil.is_some())
                    .and_then(|p| p.soil);
                let soil_delta = match (soil_now, soil_five) {
                    (Some(a), Some(b)) => Some(r3(a - b)),
                    _ => None,
                };
                (
                    rel_now.map(r3),
                    rate,
                    sustained,
                    rain15,
                    soil_now,
                    soil_delta,
                )
            }
            None => (None, None, 0, 0.0, None, None),
        };

        let inputs = Inputs {
            rel: rel_now,
            tilt_rate,
            sustained,
            rain15,
            soil,
            soil_delta,
            quality,
        };
        let (state, reasons) = decide(&inputs);
        out["rule"]["state"] = json!(state);
        out["rule"]["reasons"] = json!(reasons);
        out["rule"]["quality"] = json!(quality);
        out["rule"]["inputs"] = json!({
            "rel_pitch_deg": rel_now,
            "tilt_rate_deg_min": tilt_rate,
            "rain_15m_mm": r3(rain15),
            "soil_moisture_pct": soil,
            "soil_delta_5m_pct": soil_delta,
            "sustained_windows": sustained,
            "movement_age_s": mv_latest.map(|p| now - p.recv),
            "reference_age_s": ref_latest.map(|p| now - p.recv),
        });

        out["series"] = json!({
            "movement": movement
                .first()
                .map(|p| json!({"node_id": p.node, "points": downsample(movement_points)})),
            "reference": reference
                .first()
                .map(|p| json!({"node_id": p.node, "points": downsample(reference_points)})),
        });
        out
    }

    fn replay(&self, scenario: &str, rate: u64) -> (u16, Value) {
        let fixture = match scenario {
            "storm" => "data/replays/synthetic_storm.jsonl",
            "dry" => "data/replays/synthetic_dry_control.jsonl",
            "stable" => "data/replays/stable_dry.jsonl",
            _ => return (400, json!({"error": "unknown_scenario"})),
        };
        // One replay at a time; no time cooldown, so a demo can chain fixtures.
        {
            let mut busy = self.replay_gate.lock().unwrap_or_else(|e| e.into_inner());
            if *busy {
                return (429, json!({"error": "replay_in_progress"}));
            }
            *busy = true;
        }
        let _busy = Busy(&self.replay_gate);
        let Some(host) = receiver_addr(&self.receiver) else {
            return (400, json!({"error": "receiver_not_loopback"}));
        };
        let Ok(text) = std::fs::read_to_string(fixture) else {
            return (404, json!({"error": "fixture_missing", "detail": fixture}));
        };
        let tokens = std::fs::read_to_string(&self.tokens)
            .ok()
            .map(|t| parse_tokens(&t))
            .unwrap_or_default();
        let rate = rate.clamp(1, 60);
        let expected = 1000 / rate;
        let mut counts = (0u32, 0u32, 0u32);
        let mut errors = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let node = serde_json::from_str::<Value>(line)
                .ok()
                .and_then(|v| v["node_id"].as_str().map(str::to_string));
            let Some(node) = node else { continue };
            let Some(token) = tokens.get(&node) else {
                errors.push(format!("no token for {node}"));
                break;
            };
            let started = Instant::now();
            match post_packet(&host, token, line.trim()) {
                Ok(202) => counts.0 += 1,
                Ok(409) => counts.1 += 1,
                Ok(code) => {
                    counts.2 += 1;
                    errors.push(format!("{node}: http {code}"));
                }
                Err(e) => {
                    errors.push(format!("{node}: {e}"));
                    break;
                }
            }
            let spent = started.elapsed().as_millis() as u64;
            if spent < expected {
                thread::sleep(Duration::from_millis(expected - spent));
            }
            if counts.0 + counts.1 + counts.2 >= 400 {
                break;
            }
        }
        if counts.0 + counts.1 + counts.2 == 0 && !errors.is_empty() {
            return (
                502,
                json!({"error": "receiver_unreachable", "detail": errors[0]}),
            );
        }
        (
            200,
            json!({
                "pushed": counts.0 + counts.1 + counts.2,
                "accepted": counts.0,
                "duplicate": counts.1,
                "rejected": counts.2,
                "errors": errors
            }),
        )
    }
}

/// Releases the replay slot on every return path, including early errors.
struct Busy<'a>(&'a Mutex<bool>);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = false;
    }
}

fn parse_tokens(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let mut parts = line.split_whitespace();
            Some((parts.next()?.to_string(), parts.next()?.to_string()))
        })
        .collect()
}

fn post_packet(addr: &str, token: &str, body: &str) -> std::io::Result<u16> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    write!(
        stream,
        "POST /telemetry HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0))
}

struct Response {
    status: u16,
    body: Vec<u8>,
}

impl Response {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            body: body.to_string().into_bytes(),
        }
    }
    fn text(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Content Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn write_response(stream: &mut TcpStream, res: Response, ctype: &str) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'self'; img-src 'self' data:; \
         style-src 'self'; script-src 'self'; connect-src 'self'; \
         base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n\
         Connection: close\r\n\r\n",
        res.status,
        reason(res.status),
        ctype,
        res.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&res.body)?;
    stream.flush()
}

struct Request {
    method: String,
    path: String,
    query: String,
    body: Vec<u8>,
}

fn read_request(stream: &TcpStream) -> Result<Request, u16> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let n = reader.read_line(&mut line).map_err(|_| 400u16)?;
    if n == 0 || n > MAX_HEAD {
        return Err(400);
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target), Some(_)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(400);
    };
    let method = method.to_string();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let path = path.to_string();
    let query = query.to_string();
    let mut headers = Vec::new();
    let mut budget = MAX_HEAD;
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        budget = budget.saturating_sub(n);
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if budget == 0 {
            return Err(413);
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let len = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    if len > MAX_BODY {
        return Err(413);
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).map_err(|_| 400u16)?;
    Ok(Request {
        method,
        path,
        query,
        body,
    })
}

fn static_file(root: &str, path: &str) -> Option<(String, &'static str)> {
    let (name, ctype) = match path {
        "/" | "/index.html" => ("index.html", "text/html; charset=utf-8"),
        "/app.js" => ("app.js", "text/javascript; charset=utf-8"),
        "/style.css" => ("style.css", "text/css; charset=utf-8"),
        "/theme.js" => ("theme.js", "text/javascript; charset=utf-8"),
        "/favicon.svg" => ("favicon.svg", "image/svg+xml"),
        _ => return None,
    };
    Some((format!("{}/{name}", root.trim_end_matches('/')), ctype))
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

fn handle(app: &App, mut stream: TcpStream) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let (res, ctype) = match read_request(&stream) {
        Ok(req) => route(app, &req),
        Err(status) => (
            Response::json(status, json!({"error": reason(status)})),
            "application/json",
        ),
    };
    let _ = write_response(&mut stream, res, ctype);
}

fn route(app: &App, req: &Request) -> (Response, &'static str) {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/data") => {
            let window = query_param(&req.query, "window_s")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(900)
                .clamp(60, 21600);
            (Response::json(200, app.data(window)), "application/json")
        }
        ("POST", "/api/replay") => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or_else(|_| json!({}));
            let scenario = body["scenario"].as_str().unwrap_or("storm");
            let rate = body["rate"].as_u64().unwrap_or(20);
            let (status, value) = app.replay(scenario, rate);
            (Response::json(status, value), "application/json")
        }
        ("GET", p) => match static_file(&app.web, p) {
            Some((file, ctype)) => match std::fs::read(&file) {
                Ok(bytes) => (Response::text(200, bytes), ctype),
                Err(_) => (
                    Response::text(
                        503,
                        format!("missing {file}; run the dashboard from the repo root"),
                    ),
                    "text/plain; charset=utf-8",
                ),
            },
            None => (
                Response::json(404, json!({"error": "not_found"})),
                "application/json",
            ),
        },
        (_, "/api/data" | "/api/replay") => (
            Response::json(405, json!({"error": "method_not_allowed"})),
            "application/json",
        ),
        _ => (
            Response::json(404, json!({"error": "not_found"})),
            "application/json",
        ),
    }
}

fn main() {
    let cli = Cli::parse();
    let app = Arc::new(App {
        db: cli.db,
        web: cli.web,
        receiver: cli.receiver,
        tokens: cli.tokens,
        replay_gate: Mutex::new(false),
    });
    let listener = match TcpListener::bind(&cli.addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: bind {}: {e}", cli.addr);
            std::process::exit(1);
        }
    };
    let _ = listener.set_nonblocking(true);
    match listener.local_addr() {
        Ok(a) => println!("dashboard on http://{a} db={} web={}", app.db, app.web),
        Err(e) => eprintln!("error: {e}"),
    }
    let live = Arc::new(AtomicUsize::new(0));
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if live.load(Ordering::Relaxed) >= MAX_CONN {
                    let mut s = stream;
                    let _ = write_response(
                        &mut s,
                        Response::json(503, json!({"error": "busy"})),
                        "application/json",
                    );
                    continue;
                }
                live.fetch_add(1, Ordering::Relaxed);
                let app = Arc::clone(&app);
                let live_ref = Arc::clone(&live);
                thread::spawn(move || {
                    handle(&app, stream);
                    live_ref.fetch_sub(1, Ordering::Relaxed);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(role: &str, ts: i64, pitch: f64, recv: i64) -> P {
        P {
            node: format!("N-{role}"),
            role: role.to_string(),
            ts,
            recv,
            seq: 0,
            status: "OK".to_string(),
            batt: Some(4.0),
            rssi: Some(-70.0),
            pitch: Some(pitch),
            roll: Some(0.0),
            rain: Some(0.0),
            soil: Some(30.0),
        }
    }

    #[test]
    fn as_of_join_uses_only_past_reference_and_rejects_stale() {
        let mov = vec![
            p("movement", 1000, 1.0, 1000),
            p("movement", 1100, 1.2, 1100),
        ];
        let reference = vec![
            p("reference", 990, 0.1, 990),
            p("reference", 1095, 0.2, 1095),
        ];
        let rel = as_of_relative(&mov, &reference, 60);
        assert_eq!(rel[0].1, Some(0.9));
        assert_eq!(rel[1].1, Some(1.0));
        // reference is 105 s older than the next movement packet -> not used
        let rel = as_of_relative(&[p("movement", 1200, 2.0, 1200)], &reference, 60);
        assert_eq!(rel[0].1, None);
    }

    #[test]
    fn as_of_join_ignores_future_reference() {
        let mov = vec![p("movement", 1000, 1.0, 1000)];
        let reference = vec![p("reference", 1010, 5.0, 1010)];
        assert_eq!(as_of_relative(&mov, &reference, 60)[0].1, None);
    }

    #[test]
    fn decision_ladder() {
        let base = Inputs {
            rel: Some(0.1),
            tilt_rate: Some(0.0),
            sustained: 0,
            rain15: 0.0,
            soil: Some(30.0),
            soil_delta: Some(0.0),
            quality: "GOOD",
        };
        assert_eq!(decide(&base).0, "STABLE");
        assert_eq!(
            decide(&Inputs {
                quality: "MISSING",
                ..base
            })
            .0,
            "UNKNOWN"
        );
        assert_eq!(
            decide(&Inputs {
                quality: "DEGRADED",
                ..base
            })
            .0,
            "UNKNOWN"
        );
        let risk = Inputs {
            rel: Some(3.0),
            tilt_rate: Some(0.4),
            sustained: 4,
            ..base
        };
        assert_eq!(decide(&risk).0, "MOVEMENT_RISK");
        let flat = Inputs {
            rel: Some(3.0),
            tilt_rate: Some(0.0),
            sustained: 4,
            ..base
        };
        assert_eq!(decide(&flat).0, "WATCH");
        let rain = Inputs {
            rain15: 7.5,
            ..base
        };
        assert_eq!(decide(&rain).0, "WATCH");
        let soil = Inputs {
            soil: Some(80.0),
            ..base
        };
        assert_eq!(decide(&soil).0, "WATCH");
    }
}

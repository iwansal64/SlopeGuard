//! Read-only operator dashboard for SlopeGuard.
//!
//! Separate crate on purpose: the telemetry path in `rust/slopeguard` is owned
//! by another workstream, so this binary only *reads* the same SQLite file.
//!
//! ```text
//! slopeguard-dashboard --addr 127.0.0.1:8090 --db data/raw/slopeguard.db \
//!   --web web --receiver http://127.0.0.1:8080
//! GET  /                 static UI
//! GET  /api/data?window_s=900   status + series in one round trip
//! ```

mod forecast;
mod weather;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use clap::Parser;
use rig_core::Model;
use rig_core::completion::{CompletionRequest, Effort};
use rig_core::message::StopReason;
use rig_core::providers::gemini::{GeminiConfig, completion::GenerateContent};
use rig_reqwest::{ReqwestClient, reqwest};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use slopeguard::contract::HORIZON_MINUTES;
use slopeguard::ingest::{Store, latest_health};
use tokio::runtime::{Builder, Runtime};

const MAX_HEAD: usize = 8 * 1024;
const MAX_BODY: usize = 4 * 1024;
const MAX_CONN: usize = 24;
const ACCEPT_POLL: Duration = Duration::from_millis(1);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const REF_MAX_AGE_S: i64 = 60;
const STALE_AFTER_S: i64 = 30;
const MAX_POINTS: usize = 600;
const DEFAULT_GEMINI_MODEL: &str = "gemini-3.1-flash-lite";
const GEMINI_TIMEOUT: Duration = Duration::from_secs(10);
const GEMINI_COOLDOWN: Duration = Duration::from_secs(10);
const SITE_CONFIG: &str = "config/site.json";

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
}

struct Gemini {
    model: Model<GenerateContent>,
    runtime: Runtime,
    last_started: Mutex<Option<Instant>>,
}

impl Gemini {
    fn from_env(loopback: bool) -> Option<Self> {
        let key = std::env::var("GEMINI_API_KEY").ok()?;
        let key = key.trim();
        if key.is_empty() {
            return None;
        }
        if !loopback {
            eprintln!("Gemini disabled: bind the dashboard to a loopback address");
            return None;
        }
        let model =
            std::env::var("SG_GEMINI_MODEL").unwrap_or_else(|_| DEFAULT_GEMINI_MODEL.into());
        let model = model.trim();
        let model = model.strip_prefix("models/").unwrap_or(model);
        if !model.starts_with("gemini-")
            || model.len() > 128
            || !model
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_'))
        {
            eprintln!("Gemini disabled: invalid SG_GEMINI_MODEL");
            return None;
        }
        match Self::new(GeminiConfig::new(key.to_owned()), model, GEMINI_TIMEOUT) {
            Ok(gemini) => Some(gemini),
            Err(_) => {
                eprintln!("Gemini disabled: could not initialize the HTTP client or runtime");
                None
            }
        }
    }

    fn new(
        config: GeminiConfig,
        model: &str,
        timeout: Duration,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(timeout)
            .build()?;
        Ok(Self {
            model: config.connect(ReqwestClient::from(http)).completion(model),
            runtime: Builder::new_current_thread().enable_all().build()?,
            last_started: Mutex::new(None),
        })
    }

    fn explain(&self, rule: &Value, language: &str) -> Option<String> {
        let mut request = CompletionRequest::new(rule.to_string())
            .preamble(format!(
                "Select one of the provided SlopeGuard reason codes that best explains the existing verdict. \
                 For MOVEMENT_RISK or CRITICAL, choose relative tilt or tilt rate evidence, not environmental loading. \
                 The explanation is read in {language}. \
                 Reply with only the exact reason code, without quotation marks or Markdown."
            ))
            .max_tokens(256);
        if self.model.id() == Some(DEFAULT_GEMINI_MODEL) {
            request = request.reasoning(Effort::Minimal);
        }
        let reply = match self.runtime.block_on(self.model.call(request)) {
            Ok(reply) => reply,
            Err(error) => {
                if let Some(status) = error.provider_response().and_then(|r| r.status) {
                    eprintln!(
                        "Gemini request failed (HTTP {}): check API access, key restrictions, quota, and model",
                        status.as_u16()
                    );
                } else {
                    eprintln!("Gemini request failed ({:?})", error.kind());
                }
                return None;
            }
        };
        let text = reply.text();
        let text = text.trim();
        (reply.stop() == StopReason::Stop && !text.is_empty() && text.chars().count() <= 700)
            .then(|| text.to_owned())
    }
}

struct App {
    db: String,
    web: String,
    receiver: String,
    site: String,
    site_gate: Mutex<()>,
    gemini: Option<Gemini>,
    weather: Arc<weather::Weather>,
}

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn open_store(db: &str) -> Result<Store, String> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("{db}: {e}"))?;
    Ok(Store { conn })
}

/// Reads the operator-set BMKG area for the device. Invalid or missing files
/// mean "not configured", never a guessed location.
fn read_site(path: &str) -> Option<String> {
    let body: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let adm4 = body.get("adm4")?.as_str()?;
    weather::valid_adm4(adm4).then(|| adm4.to_owned())
}

/// Writes the location atomically so a crash cannot leave half a config file.
fn write_site(path: &str, adm4: &str) -> Result<(), String> {
    if !weather::valid_adm4(adm4) {
        return Err("invalid adm4".into());
    }
    let path = Path::new(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, format!("{}\n", json!({"adm4": adm4})))
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
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

/// Epoch second at which the newest sensor evidence goes stale, or `None`
/// when either node is faulted or already stale. Checks both the device
/// timestamp and the receive time, so a recently received backlog packet
/// cannot pass as fresh.
fn valid_until(now: i64, mv: Option<&P>, rf: Option<&P>) -> Option<i64> {
    let (mv, rf) = (mv?, rf?);
    if mv.status != "OK" || rf.status != "OK" {
        return None;
    }
    let oldest = [now - mv.recv, now - rf.recv, now - mv.ts, now - rf.ts]
        .into_iter()
        .max()?;
    (oldest <= STALE_AFTER_S).then(|| now + (STALE_AFTER_S - oldest.max(0)))
}

/// The eligible reason codes Gemini may pick from, so its choice can only
/// repeat an already-computed local fact.
fn eligible_reasons(state: &str, reasons: &[String]) -> Vec<String> {
    let tilt: Vec<String> = reasons
        .iter()
        .filter(|r| matches!(r.as_str(), "REL_TILT_ABOVE_BASELINE" | "TILT_RATE_RISING"))
        .cloned()
        .collect();
    if matches!(state, "MOVEMENT_RISK" | "CRITICAL") && !tilt.is_empty() {
        tilt
    } else {
        reasons.to_vec()
    }
}

fn state_text(state: &str, lang: &str) -> &'static str {
    match (lang, state) {
        ("en", "STABLE") => "The local rule found no abnormal movement signal.",
        ("en", "WATCH") => "The local rule sees environmental loading but no confirmed movement.",
        ("en", "MOVEMENT_RISK") => {
            "The local rule sees relative tilt above baseline and still rising."
        }
        ("en", "CRITICAL") => "The local rule reports a critical movement condition.",
        ("id", "STABLE") => "Aturan lokal tidak menemukan sinyal pergerakan abnormal.",
        ("id", "WATCH") => {
            "Aturan lokal melihat beban lingkungan, tetapi belum ada pergerakan terkonfirmasi."
        }
        ("id", "MOVEMENT_RISK") => {
            "Aturan lokal melihat tilt relatif di atas baseline dan masih naik."
        }
        ("id", "CRITICAL") => "Aturan lokal melaporkan kondisi pergerakan kritis.",
        _ => "SlopeGuard reported a local rule verdict.",
    }
}

fn reason_text(reason: &str, lang: &str) -> &'static str {
    match (lang, reason) {
        ("en", "NO_ABNORMAL_SIGNAL") => "No threshold was crossed.",
        ("en", "REL_TILT_ABOVE_BASELINE") => {
            "Relative tilt stayed at or above the 2° baseline across several readings."
        }
        ("en", "TILT_RATE_RISING") => "Tilt changed faster than the 0.2° per minute threshold.",
        ("en", "RAINFALL_LOADING") => "Rainfall reached the 15-minute loading threshold.",
        ("en", "SOIL_MOISTURE_RISING") => "Soil moisture was high or rising.",
        ("id", "NO_ABNORMAL_SIGNAL") => "Tidak ada ambang batas yang terlampaui.",
        ("id", "REL_TILT_ABOVE_BASELINE") => {
            "Tilt relatif bertahan pada atau di atas baseline 2° selama beberapa pembacaan."
        }
        ("id", "TILT_RATE_RISING") => "Tilt berubah lebih cepat dari ambang 0,2° per menit.",
        ("id", "RAINFALL_LOADING") => "Curah hujan mencapai ambang beban 15 menit.",
        ("id", "SOIL_MOISTURE_RISING") => "Kelembapan tanah tinggi atau naik.",
        _ => "",
    }
}

fn reviewed_explanation(state: &str, reason: &str, lang: &str) -> String {
    let reason = reason_text(reason, lang);
    if reason.is_empty() {
        state_text(state, lang).to_string()
    } else {
        format!("{} {reason}", state_text(state, lang))
    }
}

fn gemini_snapshot(data: &Value) -> Option<Value> {
    let rule = &data["rule"];
    (data["gemini"]["valid_until_s"]
        .as_i64()
        .is_some_and(|until| Utc::now().timestamp() < until)
        && rule["quality"] == "GOOD"
        && rule["state"] != "UNKNOWN")
        .then(|| {
            json!({
                "basis": rule["basis"],
                "state": rule["state"],
                "quality": rule["quality"],
                "horizon_minutes": rule["horizon_minutes"],
                "reasons": rule["reasons"],
            })
        })
}

fn loopback_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]")
        || host
            .strip_prefix("localhost:")
            .is_some_and(|port| port.parse::<u16>().is_ok())
        || host
            .parse::<SocketAddr>()
            .is_ok_and(|addr| addr.ip().is_loopback())
}

/// State changes are accepted only from the dashboard's own origin.
fn same_origin(req: &Request) -> bool {
    req.header("host").is_some_and(|host| {
        loopback_host(host)
            && req.header("origin").is_none_or(|origin| {
                origin.strip_prefix("http://") == Some(host)
                    || origin.strip_prefix("https://") == Some(host)
            })
    }) && req
        .header("sec-fetch-site")
        .is_none_or(|site| site == "same-origin")
}

fn json_content_type(req: &Request) -> bool {
    req.header("content-type").is_some_and(|value| {
        value
            .split(';')
            .next()
            .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
    })
}

impl App {
    fn explain(&self, req: &Request) -> (u16, Value) {
        if !same_origin(req) {
            return (403, json!({"error": "same_origin_required"}));
        }
        if !json_content_type(req) {
            return (415, json!({"error": "json_required"}));
        }
        let body: Value = serde_json::from_slice(&req.body).unwrap_or_default();
        let language = match body
            .as_object()
            .filter(|body| body.len() == 1)
            .and_then(|body| body.get("lang"))
            .and_then(Value::as_str)
        {
            Some("en") => "English",
            Some("id") => "Bahasa Indonesia",
            _ => return (400, json!({"error": "expected_language_en_or_id"})),
        };
        let Some(gemini) = &self.gemini else {
            return (503, json!({"error": "gemini_not_configured"}));
        };
        let Ok(mut last) = gemini.last_started.try_lock() else {
            return (429, json!({"error": "gemini_busy"}));
        };
        if last.is_some_and(|at| at.elapsed() < GEMINI_COOLDOWN) {
            return (429, json!({"error": "gemini_busy"}));
        }
        let data = self.data(window_seconds(&req.query));
        let Some(snapshot) = gemini_snapshot(&data) else {
            return (409, json!({"error": "unverified_data"}));
        };
        let state = snapshot["state"].as_str().unwrap_or_default().to_owned();
        let reasons: Vec<String> = snapshot["reasons"]
            .as_array()
            .map(|reasons| {
                reasons
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let eligible = eligible_reasons(&state, &reasons);
        let selection = json!({"state": state, "reasons": eligible});
        *last = Some(Instant::now());
        let Some(code) = gemini.explain(&selection, language) else {
            return (502, json!({"error": "gemini_unavailable"}));
        };
        let code = code
            .trim()
            .trim_matches(|c| matches!(c, '"' | '\'' | '`'))
            .trim();
        if !eligible.iter().any(|reason| reason == code) {
            return (502, json!({"error": "gemini_invalid_selection"}));
        }
        // The verdict can move while the model is thinking; never return an
        // explanation for a state the dashboard no longer shows.
        let latest = self.data(window_seconds(&req.query));
        if gemini_snapshot(&latest).as_ref() != Some(&snapshot) {
            return (409, json!({"error": "verdict_changed"}));
        }
        let language = if language == "Bahasa Indonesia" {
            "id"
        } else {
            "en"
        };
        (
            200,
            json!({
                "source": "gemini",
                "as_of": data["now"],
                "rule": snapshot,
                "explanation": reviewed_explanation(&state, code, language),
            }),
        )
    }

    fn data(&self, window_s: i64) -> Value {
        let at = Utc::now();
        let now = at.timestamp();
        let since = now - window_s;
        let mut out = json!({
            "now": DateTime::<Utc>::from_timestamp(now, 0).map(|d| d.to_rfc3339()),
            "window_s": window_s,
            "rule": {
                "basis": "evidence-rule",
                "horizon_minutes": HORIZON_MINUTES,
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
            "gemini": {"configured": self.gemini.is_some()},
            "weather": self.weather.snapshot(at),
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
                out["forecast"] = forecast::build(now, &[], 0.0, false, "MISSING", HORIZON_MINUTES);
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
        out["gemini"]["valid_until_s"] = json!(valid_until(now, mv_latest, ref_latest));
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

        // Environmental loading index, same thresholds the rule uses, so the
        // advisory forecast and the verdict cannot disagree about "wet".
        let loading = (rain15 / 5.0)
            .max(soil.unwrap_or(0.0) / 75.0)
            .max(soil_delta.unwrap_or(0.0) / 2.0)
            .clamp(0.0, 1.0);
        let samples: Vec<(i64, f64)> = rel
            .iter()
            .filter_map(|(t, rp, _)| rp.map(|v| (*t, v)))
            .collect();
        out["forecast"] = forecast::build(
            now,
            &samples,
            loading,
            forecast::rain_expected(&out["weather"]),
            quality,
            HORIZON_MINUTES,
        );

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

    fn site(&self, req: &Request) -> (u16, Value) {
        if !same_origin(req) {
            return (403, json!({"error": "same_origin_required"}));
        }
        if !json_content_type(req) {
            return (415, json!({"error": "json_required"}));
        }
        let body: Value = serde_json::from_slice(&req.body).unwrap_or_default();
        let adm4 = body
            .as_object()
            .filter(|body| body.len() == 1)
            .and_then(|body| body.get("adm4"))
            .and_then(Value::as_str);
        let Some(adm4) = adm4.filter(|adm4| weather::valid_adm4(adm4)) else {
            return (400, json!({"error": "expected_adm4"}));
        };
        let _gate = self.site_gate.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(error) = write_site(&self.site, adm4) {
            eprintln!("site config write failed: {error}");
            return (500, json!({"error": "site_write_failed"}));
        }
        self.weather.set_adm4(adm4.to_owned());
        (200, json!({"weather": self.weather.snapshot(Utc::now())}))
    }
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
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
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
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
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
        headers,
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

fn window_seconds(query: &str) -> i64 {
    query_param(query, "window_s")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(900)
        .clamp(60, 21600)
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
        ("GET", "/api/data") => (
            Response::json(200, app.data(window_seconds(&req.query))),
            "application/json",
        ),
        ("POST", "/api/explain") => {
            let (status, value) = app.explain(req);
            (Response::json(status, value), "application/json")
        }
        ("POST", "/api/site") => {
            let (status, value) = app.site(req);
            (Response::json(status, value), "application/json")
        }
        (_, "/api/explain") => (
            Response::json(405, json!({"error": "method_not_allowed"})),
            "application/json",
        ),
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
        (_, "/api/data" | "/api/site") => (
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
    let listener = match TcpListener::bind(&cli.addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: bind {}: {e}", cli.addr);
            std::process::exit(1);
        }
    };
    let gemini = Gemini::from_env(
        listener
            .local_addr()
            .is_ok_and(|addr| addr.ip().is_loopback()),
    );
    let adm4 = read_site(SITE_CONFIG).or_else(|| {
        let adm4 = std::env::var("SG_BMKG_ADM4").ok()?;
        if weather::valid_adm4(&adm4) {
            Some(adm4)
        } else {
            eprintln!("BMKG disabled: SG_BMKG_ADM4 must look like 31.71.03.1001");
            None
        }
    });
    let app = Arc::new(App {
        db: cli.db,
        web: cli.web,
        receiver: cli.receiver,
        site: SITE_CONFIG.into(),
        site_gate: Mutex::new(()),
        gemini,
        weather: weather::Weather::start(adm4),
    });
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
    use slopeguard::ingest::ingest_lines;
    use slopeguard::replay::build_stable_dry;

    #[test]
    fn weather_status_is_reported_without_sensor_data() {
        let app = App {
            db: ":memory:".into(),
            web: String::new(),
            receiver: String::new(),
            site: String::new(),
            site_gate: Mutex::new(()),
            gemini: None,
            weather: Arc::new(weather::Weather::disabled()),
        };
        let data = app.data(1800);
        assert_eq!(data["weather"], json!({"status": "disabled"}));
        assert_eq!(data["rule"]["state"], "UNKNOWN");
        assert_eq!(data["forecast"]["quality"], "MISSING");
        assert_eq!(data["forecast"]["values"].as_array().unwrap().len(), 0);
    }

    fn mock_gemini(status: u16, body: Value) -> (Gemini, thread::JoinHandle<Request>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let config = GeminiConfig::new("test-key")
            .with_base_url(format!("http://{}", listener.local_addr().unwrap()));
        let gemini = Gemini::new(config, DEFAULT_GEMINI_MODEL, Duration::from_secs(3)).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(IO_TIMEOUT)).unwrap();
            let req = read_request(&stream).unwrap();
            write_response(
                &mut stream,
                Response::json(status, body),
                "application/json",
            )
            .unwrap();
            req
        });
        (gemini, server)
    }

    #[test]
    fn site_location_round_trips_and_rejects_invalid_codes() {
        let dir = std::env::temp_dir().join(format!(
            "slopeguard-site-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap(),
        ));
        std::fs::create_dir(&dir).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let dir = Cleanup(dir);
        let path = dir.0.join("site.json").to_str().unwrap().to_owned();

        assert_eq!(read_site(&path), None);
        for invalid in ["", "../../etc/passwd", "31.71.3.1001", "31.71.03.1001x"] {
            assert!(write_site(&path, invalid).is_err());
            assert_eq!(read_site(&path), None);
        }
        write_site(&path, "31.71.03.1001").unwrap();
        assert_eq!(read_site(&path).as_deref(), Some("31.71.03.1001"));
        std::fs::write(&path, "{").unwrap();
        assert_eq!(read_site(&path), None);
    }

    #[test]
    fn site_endpoint_saves_location_and_weather_follows_it() {
        let dir = std::env::temp_dir().join(format!(
            "slopeguard-site-api-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap(),
        ));
        std::fs::create_dir(&dir).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let dir = Cleanup(dir);
        let path = dir.0.join("site.json").to_str().unwrap().to_owned();
        let app = App {
            db: ":memory:".into(),
            web: String::new(),
            receiver: String::new(),
            site: path.clone(),
            site_gate: Mutex::new(()),
            gemini: None,
            weather: Arc::new(weather::Weather::disabled()),
        };
        let mut req = Request {
            method: "POST".into(),
            path: "/api/site".into(),
            query: String::new(),
            headers: vec![
                ("host".into(), "127.0.0.1:8090".into()),
                ("content-type".into(), "application/json".into()),
                ("origin".into(), "http://127.0.0.1:8090".into()),
            ],
            body: br#"{"adm4":"31.71.03.1001"}"#.to_vec(),
        };

        req.headers[2].1 = "https://example.invalid".into();
        assert_eq!(route(&app, &req).0.status, 403);
        req.headers[2].1 = "http://127.0.0.1:8090".into();
        req.body = br#"{"adm4":"../../etc/passwd"}"#.to_vec();
        assert_eq!(route(&app, &req).0.status, 400);
        req.body = br#"{"adm4":"31.71.03.1001","extra":1}"#.to_vec();
        assert_eq!(route(&app, &req).0.status, 400);
        assert_eq!(read_site(&path), None);

        req.body = br#"{"adm4":"31.71.03.1001"}"#.to_vec();
        let res = route(&app, &req).0;
        assert_eq!(res.status, 200);
        let body: Value = serde_json::from_slice(&res.body).unwrap();
        assert_eq!(body["weather"]["status"], "loading");
        assert_eq!(body["weather"]["adm4"], "31.71.03.1001");
        assert_eq!(read_site(&path).as_deref(), Some("31.71.03.1001"));
        assert_eq!(app.data(1800)["weather"]["adm4"], "31.71.03.1001");
    }

    #[test]
    fn gemini_explains_local_verdict_and_fails_without_changing_it() {
        let dir = std::env::temp_dir().join(format!(
            "slopeguard-dashboard-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap(),
        ));
        std::fs::create_dir(&dir).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let dir = Cleanup(dir);
        let db = dir.0.join("raw.db").to_str().unwrap().to_owned();
        let mut store = Store::open(&db).unwrap();
        let mut app = App {
            db,
            web: String::new(),
            receiver: String::new(),
            site: String::new(),
            site_gate: Mutex::new(()),
            gemini: None,
            weather: Arc::new(weather::Weather::disabled()),
        };
        let mut req = Request {
            method: "POST".into(),
            path: "/api/explain".into(),
            query: "window_s=1800".into(),
            headers: vec![
                ("host".into(), "127.0.0.1:8090".into()),
                ("content-type".into(), "application/json".into()),
                ("origin".into(), "http://127.0.0.1:8090".into()),
            ],
            body: br#"{"lang":"id"}"#.to_vec(),
        };
        assert_eq!(route(&app, &req).0.status, 503);
        assert_eq!(app.data(1800)["gemini"]["configured"], false);

        let text = "NO_ABNORMAL_SIGNAL";
        let (gemini, server) = mock_gemini(
            200,
            json!({
                "candidates": [{"content": {"role": "model", "parts": [{"text": text}]}, "finishReason": "STOP"}]
            }),
        );
        app.gemini = Some(gemini);
        assert_eq!(route(&app, &req).0.status, 409);

        let packets = build_stable_dry(1).into_iter().map(|mut packet| {
            packet["ts_device"] = json!(Utc::now().to_rfc3339());
            packet.to_string()
        });
        assert_eq!(
            ingest_lines(&mut store, packets, "test").unwrap().accepted,
            2
        );
        let status = app.data(1800);
        assert_eq!(status["rule"]["state"], "STABLE");
        assert_eq!(status["gemini"]["configured"], true);
        assert_eq!(status["model"]["available"], false);
        req.headers[2].1 = "https://example.invalid".into();
        assert_eq!(route(&app, &req).0.status, 403);
        req.headers[2].1 = "http://127.0.0.1:8090".into();
        req.body = br#"{"lang":"id","prompt":"Ignored input"}"#.to_vec();
        assert_eq!(route(&app, &req).0.status, 400);
        req.body = br#"{"lang":"id"}"#.to_vec();
        let res = route(&app, &req).0;
        assert_eq!(res.status, 200);
        let body: Value = serde_json::from_slice(&res.body).unwrap();
        let expected_rule = json!({
            "basis": "evidence-rule", "state": "STABLE", "quality": "GOOD",
            "horizon_minutes": HORIZON_MINUTES, "reasons": ["NO_ABNORMAL_SIGNAL"],
        });
        assert_eq!(body["source"], "gemini");
        assert_eq!(
            body["explanation"],
            reviewed_explanation("STABLE", "NO_ABNORMAL_SIGNAL", "id")
        );
        assert_eq!(body["rule"], expected_rule);
        assert_eq!(route(&app, &req).0.status, 429);

        let sent = server.join().unwrap();
        assert_eq!(sent.method, "POST");
        assert_eq!(
            sent.path,
            format!("/v1beta/models/{DEFAULT_GEMINI_MODEL}:generateContent")
        );
        assert!(sent.query.is_empty());
        assert_eq!(sent.header("x-goog-api-key"), Some("test-key"));
        let payload: Value = serde_json::from_slice(&sent.body).unwrap();
        assert!(payload["tools"].is_null());
        assert_eq!(payload["generationConfig"]["maxOutputTokens"], 256);
        let context: Value =
            serde_json::from_str(payload["contents"][0]["parts"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(
            context,
            json!({"state": "STABLE", "reasons": ["NO_ABNORMAL_SIGNAL"]})
        );
        assert!(
            payload["systemInstruction"]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Bahasa Indonesia")
        );

        let (gemini, server) = mock_gemini(
            403,
            json!({"error": {"message": "Private provider response"}}),
        );
        app.gemini = Some(gemini);
        let res = route(&app, &req).0;
        assert_eq!(res.status, 502);
        assert_eq!(
            serde_json::from_slice::<Value>(&res.body).unwrap(),
            json!({"error": "gemini_unavailable"})
        );
        server.join().unwrap();
        assert_eq!(app.data(1800)["rule"]["state"], "STABLE");
    }

    #[test]
    fn stale_or_faulted_evidence_disables_gemini_and_old_fixtures_do_not_qualify() {
        let now = Utc::now().timestamp();
        let fresh = |status: &str| P {
            status: status.to_string(),
            ..p("movement", now, 1.0, now)
        };
        let until = valid_until(now, Some(&fresh("OK")), Some(&fresh("OK")));
        assert_eq!(until, Some(now + STALE_AFTER_S));
        assert_eq!(
            valid_until(now, Some(&fresh("FAULT")), Some(&fresh("OK"))),
            None
        );
        assert_eq!(valid_until(now, Some(&fresh("OK")), None), None);
        let stale = P {
            recv: now - STALE_AFTER_S - 5,
            ..fresh("OK")
        };
        assert_eq!(valid_until(now, Some(&stale), Some(&fresh("OK"))), None);
        // A backlog packet received now but stamped long ago is not fresh.
        let backlog = P {
            ts: now - 3600,
            ..fresh("OK")
        };
        assert_eq!(valid_until(now, Some(&backlog), Some(&fresh("OK"))), None);

        let data = json!({
            "rule": {"basis": "evidence-rule", "state": "STABLE", "quality": "GOOD",
                     "horizon_minutes": HORIZON_MINUTES, "reasons": ["NO_ABNORMAL_SIGNAL"]},
            "gemini": {"configured": true, "valid_until_s": now - 1},
        });
        assert_eq!(gemini_snapshot(&data), None);
    }

    #[test]
    fn movement_risk_only_offers_movement_evidence() {
        let reasons = [
            "REL_TILT_ABOVE_BASELINE",
            "TILT_RATE_RISING",
            "RAINFALL_LOADING",
        ]
        .map(String::from);
        assert_eq!(
            eligible_reasons("MOVEMENT_RISK", &reasons),
            vec![
                "REL_TILT_ABOVE_BASELINE".to_string(),
                "TILT_RATE_RISING".to_string()
            ]
        );
        assert_eq!(eligible_reasons("WATCH", &reasons).len(), 3);
    }

    #[test]
    fn fresh_history_yields_thirty_minute_forecast_in_api_data() {
        let dir = std::env::temp_dir().join(format!(
            "slopeguard-forecast-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap(),
        ));
        std::fs::create_dir(&dir).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let dir = Cleanup(dir);
        let db = dir.0.join("raw.db").to_str().unwrap().to_owned();
        let mut store = Store::open(&db).unwrap();
        let now = Utc::now();
        // 61 packets at 10 s spacing span the 10-minute trend window.
        let packets = build_stable_dry(61).into_iter().map(|mut packet| {
            let i: i64 = packet["seq"].as_i64().unwrap() - 1;
            packet["ts_device"] =
                json!((now - chrono::TimeDelta::seconds(10 * (60 - i))).to_rfc3339());
            packet.to_string()
        });
        assert_eq!(
            ingest_lines(&mut store, packets, "test").unwrap().accepted,
            122
        );
        let app = App {
            db,
            web: String::new(),
            receiver: String::new(),
            site: String::new(),
            site_gate: Mutex::new(()),
            gemini: None,
            weather: Arc::new(weather::Weather::disabled()),
        };
        let data = app.data(1800);
        assert_eq!(data["rule"]["quality"], "GOOD");
        let forecast = &data["forecast"];
        assert_eq!(forecast["quality"], "GOOD");
        assert_eq!(forecast["basis"], "damped-trend+v1");
        assert_eq!(forecast["horizon_minutes"], HORIZON_MINUTES);
        let values = forecast["values"].as_array().unwrap();
        assert_eq!(values.len(), 30);
        assert_eq!(values[0]["t_plus_min"], 1);
        assert_eq!(values[29]["t_plus_min"], 30);
        for v in values {
            assert!(v["band_deg"].as_f64().unwrap() >= 0.0);
            assert!(v["risk"].as_f64().is_some_and(|r| (0.0..=1.0).contains(&r)));
            assert!(["STABLE", "WATCH", "MOVEMENT_RISK"].contains(&v["state"].as_str().unwrap()));
        }
    }

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

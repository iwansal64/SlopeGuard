//! Telemetry receiver: one packet per HTTP POST, per-node bearer token.
//!
//! stdlib only: `std::net` speaks HTTP/1.1 with `Connection: close`. TLS is
//! terminated by a reverse proxy in deployment; the pilot listener is plain
//! HTTP bound to localhost. Never expose this port directly.
//!
//! ```text
//! slopeguard serve --addr 127.0.0.1:8080 --db data/raw/slopeguard.db --tokens config/nodes.tokens
//! POST /telemetry  Authorization: Bearer <node token>       -> 202 | 401 | 409 | 413 | 422
//! GET  /health     Authorization: Bearer <any node token>   -> 200 | 401
//! ```

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

use crate::ingest::{Store, ingest_lines, last_reject_reason, latest_health};

pub const MAX_HEAD: usize = 16 * 1024;
pub const MAX_BODY: usize = 8 * 1024;
const MIN_TOKEN_LEN: usize = 16;
const ACCEPT_POLL: Duration = Duration::from_millis(10);
const IO_TIMEOUT: Duration = Duration::from_secs(5);

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// `NODE_ID -> bearer token`. A token authenticates exactly one node.
#[derive(Debug, Default)]
pub struct Tokens {
    map: HashMap<String, String>,
}

impl Tokens {
    /// One `<NODE_ID> <token>` per line; blank lines and `#` comments ignored.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut map = HashMap::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (Some(node), Some(token), None) = (parts.next(), parts.next(), parts.next()) else {
                return Err(format!("line {}: expected `<NODE_ID> <token>`", i + 1));
            };
            if token.len() < MIN_TOKEN_LEN {
                return Err(format!(
                    "line {}: token shorter than {MIN_TOKEN_LEN} chars",
                    i + 1
                ));
            }
            if map.insert(node.to_string(), token.to_string()).is_some() {
                return Err(format!("line {}: duplicate node {node}", i + 1));
            }
        }
        if map.is_empty() {
            return Err("no node tokens".to_string());
        }
        Ok(Self { map })
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::parse(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The node this token authenticates, if any.
    pub fn node_for(&self, token: &str) -> Option<&str> {
        self.map
            .iter()
            .find(|(_, t)| constant_time_eq(t.as_bytes(), token.as_bytes()))
            .map(|(node, _)| node.as_str())
    }
}

struct Response {
    status: u16,
    body: String,
}

impl Response {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            body: body.to_string(),
        }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Content Too Large",
        422 => "Unprocessable Entity",
        431 => "Request Header Fields Too Large",
        _ => "Internal Server Error",
    }
}

fn write_response(stream: &mut TcpStream, res: Response) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        res.status,
        reason(res.status),
        res.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(res.body.as_bytes())?;
    stream.flush()
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// Read one request. Status codes come back as `Err` so the caller still writes
/// a response before closing: oversized or malformed requests are never silent
/// drops.
fn read_request(stream: &TcpStream) -> Result<Request, u16> {
    let mut reader = BufReader::new(stream);
    let mut budget = MAX_HEAD;
    let mut line = String::new();

    let n = reader.read_line(&mut line).map_err(|_| 400u16)?;
    if n == 0 || n > budget {
        return Err(400);
    }
    budget -= n;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(path), Some(_version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(400);
    };
    let method = method.to_string();
    let path = path.split('?').next().unwrap_or("/").to_string();

    let mut headers = Vec::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        if n > budget {
            return Err(431);
        }
        budget -= n;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        let Some((k, v)) = trimmed.split_once(':') else {
            return Err(400);
        };
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }

    let len = match header(&headers, "content-length") {
        Some(v) => v.parse::<usize>().map_err(|_| 400u16)?,
        None => 0,
    };
    if len > MAX_BODY {
        return Err(413);
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).map_err(|_| 400u16)?;
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

pub struct Server {
    listener: TcpListener,
    store: Store,
    tokens: Tokens,
    stop: Arc<AtomicBool>,
}

pub struct ServeHandle {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

impl ServeHandle {
    /// Stop accepting and join the thread, releasing the SQLite handle.
    pub fn shutdown(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.join.join();
    }
}

impl Server {
    pub fn bind(addr: &str, store: Store, tokens: Tokens) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            store,
            tokens,
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serve until the stop flag is set. Sequential on purpose: two nodes at one
    /// packet per 10 s do not need a thread pool.
    pub fn serve(mut self) {
        while !self.stop.load(Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((stream, _)) => self.handle(stream),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(ACCEPT_POLL),
                Err(_) => break,
            }
        }
    }

    /// Serve on a background thread; tests use this, the CLI calls `serve`.
    pub fn spawn(self) -> std::io::Result<ServeHandle> {
        let addr = self.local_addr()?;
        let stop = Arc::clone(&self.stop);
        let join = thread::spawn(move || self.serve());
        Ok(ServeHandle { addr, stop, join })
    }

    fn handle(&mut self, mut stream: TcpStream) {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        let res = match read_request(&stream) {
            Ok(req) => self.route(req),
            Err(status) => Response::json(status, json!({"error": reason(status)})),
        };
        let _ = write_response(&mut stream, res);
    }

    fn route(&mut self, req: Request) -> Response {
        match (req.method.as_str(), req.path.as_str()) {
            ("POST", "/telemetry") => self.post_telemetry(&req),
            ("GET", "/health") => self.health(&req),
            (_, "/telemetry" | "/health") => {
                Response::json(405, json!({"error": "method_not_allowed"}))
            }
            _ => Response::json(404, json!({"error": "not_found"})),
        }
    }

    fn authenticate(&self, headers: &[(String, String)]) -> Option<String> {
        let value = header(headers, "authorization")?;
        let token = value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "))?;
        self.tokens.node_for(token.trim()).map(str::to_string)
    }

    fn post_telemetry(&mut self, req: &Request) -> Response {
        let Some(node) = self.authenticate(&req.headers) else {
            return Response::json(401, json!({"error": "unauthorized"}));
        };
        let text = match std::str::from_utf8(&req.body) {
            Ok(t) if !t.trim().is_empty() => t.trim(),
            _ => {
                return Response::json(
                    422,
                    json!({"error": "json", "detail": "empty or non-UTF-8 body"}),
                );
            }
        };
        let packet: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                return Response::json(422, json!({"error": "json", "detail": e.to_string()}));
            }
        };
        // A token authorizes exactly one node. A mismatch gets the same answer
        // as an unknown token: no detail leaks about which check failed.
        if packet["node_id"].as_str() != Some(node.as_str()) {
            return Response::json(401, json!({"error": "unauthorized"}));
        }
        let report = match ingest_lines(&mut self.store, [text], "http") {
            Ok(r) => r,
            Err(e) => {
                return Response::json(500, json!({"error": "store", "detail": e.to_string()}));
            }
        };
        if report.duplicates > 0 {
            return Response::json(
                409,
                json!({"error": "duplicate", "reason": "duplicate:seq"}),
            );
        }
        if report.rejected > 0 {
            let detail = last_reject_reason(&self.store)
                .ok()
                .flatten()
                .unwrap_or_else(|| "contract".to_string());
            return Response::json(422, json!({"error": "contract", "reason": detail}));
        }
        Response::json(
            202,
            json!({"accepted": true, "node_id": node, "seq": packet["seq"]}),
        )
    }

    fn health(&self, req: &Request) -> Response {
        if self.authenticate(&req.headers).is_none() {
            return Response::json(401, json!({"error": "unauthorized"}));
        }
        match latest_health(&self.store) {
            Ok(rows) => {
                let nodes: Vec<Value> = rows
                    .into_iter()
                    .map(|h| {
                        json!({
                            "site_id": h.site_id,
                            "node_id": h.node_id,
                            "node_role": h.node_role,
                            "seq": h.seq,
                            "ts_device": h.ts_device,
                            "received_at": h.received_at,
                            "sensor_status": h.sensor_status,
                            "battery_v": h.battery_v,
                            "signal_dbm": h.signal_dbm,
                        })
                    })
                    .collect();
                let count = nodes.len();
                Response::json(200, json!({"count": count, "nodes": nodes}))
            }
            Err(e) => Response::json(500, json!({"error": "store", "detail": e.to_string()})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Tokens, constant_time_eq};

    const T1: &str = "0123456789abcdef";
    const T2: &str = "fedcba9876543210";

    #[test]
    fn parses_tokens_and_maps_by_constant_time() {
        let t = Tokens::parse(&format!("IMU-MOVE-01 {T1}\nIMU-REF-01 {T2}\n")).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t.node_for(T1), Some("IMU-MOVE-01"));
        assert_eq!(t.node_for(T2), Some("IMU-REF-01"));
        assert_eq!(t.node_for("nope"), None);
        assert_eq!(t.node_for(&T1[..15]), None);
    }

    #[test]
    fn rejects_short_duplicate_and_malformed_lines() {
        assert!(Tokens::parse("IMU-MOVE-01 short\n").is_err());
        assert!(Tokens::parse(&format!("A_1 {T1}\nA_1 {T2}\n")).is_err());
        assert!(Tokens::parse(&format!("IMU-MOVE-01 {T1} extra\n")).is_err());
        assert!(Tokens::parse("# only comments\n").is_err());
    }

    #[test]
    fn constant_time_eq_matches_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}

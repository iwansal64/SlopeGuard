mod common;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};

use common::TempDir;
use serde_json::Value;
use slopeguard::ingest::Store;
use slopeguard::receiver::{ServeHandle, Server, Tokens};
use slopeguard::replay::{build_stable_dry, python_json};

const TOK_MOVE: &str = "move-token-0123456789";
const TOK_REF: &str = "ref-token-9876543210";

fn start() -> (ServeHandle, TempDir) {
    let tmp = TempDir::new("receiver");
    let store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let tokens = Tokens::parse(&format!("IMU-MOVE-01 {TOK_MOVE}\nIMU-REF-01 {TOK_REF}\n")).unwrap();
    let server = Server::bind("127.0.0.1:0", store, tokens).unwrap();
    (server.spawn().unwrap(), tmp)
}

fn request(addr: SocketAddr, raw: &[u8]) -> (u16, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(raw).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

fn post(addr: SocketAddr, token: &str, packet: &str) -> (u16, String) {
    let raw = format!(
        "POST /telemetry HTTP/1.1\r\nHost: t\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{packet}",
        packet.len()
    );
    request(addr, raw.as_bytes())
}

fn get(addr: SocketAddr, token: &str, path: &str) -> (u16, String) {
    let raw = format!("GET {path} HTTP/1.1\r\nHost: t\r\nAuthorization: Bearer {token}\r\n\r\n");
    request(addr, raw.as_bytes())
}

fn token_for(packet: &Value) -> &'static str {
    if packet["node_id"] == "IMU-MOVE-01" {
        TOK_MOVE
    } else {
        TOK_REF
    }
}

#[test]
fn twenty_packets_accepted_and_health_reports_both_nodes() {
    let (handle, tmp) = start();
    for p in build_stable_dry(20) {
        let (status, body) = post(handle.addr, token_for(&p), &python_json(&p));
        assert_eq!(status, 202, "{body}");
    }

    let (status, body) = get(handle.addr, TOK_MOVE, "/health");
    assert_eq!(status, 200);
    let health: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(health["count"].as_u64(), Some(2));
    let nodes = health["nodes"].as_array().unwrap();
    let mut seqs: Vec<i64> = nodes.iter().map(|n| n["seq"].as_i64().unwrap()).collect();
    seqs.sort();
    assert_eq!(seqs, vec![20, 20]);

    handle.shutdown();
    // After the server drops its handle, the store has 40 raw packets.
    let store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let n: i64 = store
        .conn
        .query_row("SELECT count(*) FROM raw_packet", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 40);
    let rejected: i64 = store
        .conn
        .query_row("SELECT count(*) FROM rejected", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rejected, 0);
}

#[test]
fn duplicate_returns_409() {
    let (handle, _tmp) = start();
    let body = python_json(&build_stable_dry(1)[0]);
    assert_eq!(post(handle.addr, TOK_MOVE, &body).0, 202);
    let (status, out) = post(handle.addr, TOK_MOVE, &body);
    assert_eq!(status, 409);
    assert!(out.contains("duplicate"), "{out}");
}

#[test]
fn missing_or_unknown_token_is_401() {
    let (handle, _tmp) = start();
    let body = python_json(&build_stable_dry(1)[0]);
    let raw = format!(
        "POST /telemetry HTTP/1.1\r\nHost: t\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    assert_eq!(request(handle.addr, raw.as_bytes()).0, 401);
    assert_eq!(post(handle.addr, "wrong-token-00000000", &body).0, 401);
}

#[test]
fn token_for_another_node_is_401() {
    let (handle, _tmp) = start();
    let body = python_json(&build_stable_dry(1)[0]); // IMU-MOVE-01 packet
    assert_eq!(post(handle.addr, TOK_REF, &body).0, 401);
}

#[test]
fn contract_violation_is_422_with_reason() {
    let (handle, _tmp) = start();
    let mut p = build_stable_dry(1)[0].clone();
    p.as_object_mut().unwrap().remove("battery_v");
    let (status, body) = post(handle.addr, TOK_MOVE, &python_json(&p));
    assert_eq!(status, 422);
    assert!(body.contains("missing:battery_v"), "{body}");
}

#[test]
fn malformed_json_is_422() {
    let (handle, _tmp) = start();
    let (status, body) = post(handle.addr, TOK_MOVE, "{not json");
    assert_eq!(status, 422);
    assert!(body.contains("\"json\""), "{body}");
}

#[test]
fn oversized_body_is_413() {
    let (handle, _tmp) = start();
    let raw = format!(
        "POST /telemetry HTTP/1.1\r\nHost: t\r\nAuthorization: Bearer {TOK_MOVE}\r\nContent-Length: 65536\r\n\r\n"
    );
    assert_eq!(request(handle.addr, raw.as_bytes()).0, 413);
}

#[test]
fn routing_and_health_auth() {
    let (handle, _tmp) = start();
    assert_eq!(get(handle.addr, TOK_MOVE, "/nope").0, 404);
    assert_eq!(get(handle.addr, TOK_MOVE, "/telemetry").0, 405);
    let raw = "GET /health HTTP/1.1\r\nHost: t\r\n\r\n";
    assert_eq!(request(handle.addr, raw.as_bytes()).0, 401);
}

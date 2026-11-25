mod common;

use common::TempDir;
use serde_json::Value;
use slopeguard::ingest::{Store, ingest_lines, latest_health, load_jsonl};
use slopeguard::replay::{build_stable_dry, python_json};

fn lines(packets: &[Value]) -> Vec<String> {
    packets.iter().map(python_json).collect()
}

#[test]
fn twenty_packets_accepted() {
    let tmp = TempDir::new("twenty");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let report = ingest_lines(&mut store, lines(&build_stable_dry(20)), "test").unwrap();
    assert_eq!(report.accepted, 40);
    assert_eq!(report.rejected, 0);
    let n: i64 = store
        .conn
        .query_row("SELECT count(*) FROM raw_packet", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 40);
}

#[test]
fn raw_payload_preserved_byte_for_byte() {
    let tmp = TempDir::new("raw");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let raw = r#"{"node_id":"IMU-MOVE-01","seq":7}"#;
    ingest_lines(&mut store, [raw], "test").unwrap();
    let payload: String = store
        .conn
        .query_row("SELECT payload FROM rejected", [], |r| r.get(0))
        .unwrap();
    assert_eq!(payload, raw);
}

#[test]
fn accepts_compact_and_spaced_json() {
    let tmp = TempDir::new("spacing");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let packets = build_stable_dry(1);
    let compact = serde_json::to_string(&packets[0]).unwrap();
    let spaced = python_json(&packets[1]);
    let report = ingest_lines(&mut store, [compact, spaced], "t").unwrap();
    assert_eq!(report.accepted, 2);
}

#[test]
fn malformed_logged_not_crashing() {
    let tmp = TempDir::new("malformed");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let report = ingest_lines(&mut store, ["{not json"], "test").unwrap();
    assert_eq!((report.accepted, report.rejected), (0, 1));
    let reason: String = store
        .conn
        .query_row("SELECT reason FROM rejected", [], |r| r.get(0))
        .unwrap();
    assert!(reason.starts_with("json:"));
}

#[test]
fn rejects_survive_reopen() {
    let tmp = TempDir::new("reopen");
    let path = tmp.path().join("raw.db");
    {
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        ingest_lines(&mut store, ["{not json"], "test").unwrap();
    }
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let rows: Vec<String> = store
        .conn
        .prepare("SELECT reason FROM rejected")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].starts_with("json:"));
}

#[test]
fn invalid_packet_logged() {
    let tmp = TempDir::new("invalid");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let mut packet = build_stable_dry(1)[0].clone();
    packet
        .as_object_mut()
        .unwrap()
        .insert("tilt_pitch_deg".into(), 400.into());
    let report = ingest_lines(&mut store, [python_json(&packet)], "test").unwrap();
    assert_eq!(report.rejected, 1);
}

#[test]
fn null_required_is_a_contract_reject_not_a_duplicate() {
    let tmp = TempDir::new("nullrequired");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let mut packet = build_stable_dry(1)[0].clone();
    packet
        .as_object_mut()
        .unwrap()
        .insert("node_role".into(), Value::Null);
    let report = ingest_lines(&mut store, [python_json(&packet)], "test").unwrap();
    assert_eq!((report.rejected, report.duplicates), (1, 0));
    let reason: String = store
        .conn
        .query_row("SELECT reason FROM rejected", [], |r| r.get(0))
        .unwrap();
    assert_eq!(reason, "contract:missing:node_role");
}

#[test]
fn duplicate_seq_rejected() {
    let tmp = TempDir::new("dup");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let packet = python_json(&build_stable_dry(1)[0]);
    ingest_lines(&mut store, [packet.clone()], "t").unwrap();
    let report = ingest_lines(&mut store, [packet], "t").unwrap();
    assert_eq!(report.accepted, 0);
    assert_eq!(report.duplicates, 1);
    assert_eq!(report.out_of_order, 0);
}

#[test]
fn duplicate_allowed_across_nodes() {
    let tmp = TempDir::new("dupnodes");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let packets = build_stable_dry(1);
    assert_eq!(packets[0]["seq"], packets[1]["seq"]);
    let report = ingest_lines(&mut store, lines(&packets), "t").unwrap();
    assert_eq!(report.accepted, 2);
}

#[test]
fn out_of_order_counted_and_kept() {
    let tmp = TempDir::new("ooo");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let mut a = build_stable_dry(1)[0].clone();
    a.as_object_mut().unwrap().insert("seq".into(), 5.into());
    let mut b = build_stable_dry(1)[0].clone();
    b.as_object_mut().unwrap().insert("seq".into(), 4.into());
    let report = ingest_lines(&mut store, [python_json(&a), python_json(&b)], "t").unwrap();
    assert_eq!(report.accepted, 2);
    assert_eq!(report.out_of_order, 1);
}

#[test]
fn latest_health_pairs_movement_and_reference() {
    let tmp = TempDir::new("health");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    ingest_lines(&mut store, lines(&build_stable_dry(20)), "t").unwrap();
    let health = latest_health(&store).unwrap();
    assert_eq!(health.len(), 2);
    let movement = health.iter().find(|h| h.node_role == "movement").unwrap();
    let reference = health.iter().find(|h| h.node_role == "reference").unwrap();
    assert_eq!(movement.seq, 20);
    assert_eq!(reference.seq, 20);
    assert_eq!(movement.sensor_status, "OK");
}

#[test]
fn blank_lines_skipped() {
    let tmp = TempDir::new("blank");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let packet = python_json(&build_stable_dry(1)[0]);
    let report =
        ingest_lines(&mut store, ["".to_string(), packet, "   ".to_string()], "t").unwrap();
    assert_eq!(report.accepted, 1);
    assert_eq!(report.rejected, 0);
}

#[test]
fn load_jsonl_filters_blank_lines() {
    let tmp = TempDir::new("load");
    let path = tmp.path().join("in.jsonl");
    std::fs::write(&path, "{\"a\":1}\n\n{\"a\":2}\n").unwrap();
    assert_eq!(
        load_jsonl(path.to_str().unwrap()).unwrap(),
        vec!["{\"a\":1}", "{\"a\":2}"]
    );
}

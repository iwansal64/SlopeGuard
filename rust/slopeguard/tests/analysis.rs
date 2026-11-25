mod common;

use common::TempDir;
use serde_json::Value;
use slopeguard::analysis::{
    BaselineConfig, FEATURE_HEADER, LABEL_EXTRA_HEADER, build_features, generate_labels,
    write_baseline_results, write_features_csv, write_split_manifest,
};
use slopeguard::ingest::{Store, ingest_lines};
use slopeguard::replay::{build_stable_dry, python_json};

fn ingest_packets(store: &mut Store, packets: &[Value]) {
    let lines: Vec<String> = packets.iter().map(python_json).collect();
    ingest_lines(store, lines, "test").unwrap();
}

fn feature_line(ts: &str, rain_15: &str, soil: &str, rate: &str) -> String {
    format!(
        "SLOPE-001,IMU-MOVE-01,{ts},2.0,0.0,2.0,0.0,{rate},1.0,0.0,{rain_15},0.0,{soil},0.0,0,0,0,false,false,false"
    )
}

fn labels_csv(rows: &[(&str, &str, &str, &str)]) -> String {
    let mut out = format!("{FEATURE_HEADER},{LABEL_EXTRA_HEADER}\n");
    for (ts, event, label, status) in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            feature_line(ts, "6.0", "80.0", "1.0"),
            event,
            label,
            status,
            "independent_ground_truth",
            1.0,
            30
        ));
    }
    out
}

#[test]
fn feature_windows_never_read_future_packets() {
    let tmp = TempDir::new("causal-features");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let mut packets = build_stable_dry(3);
    packets[2]
        .as_object_mut()
        .unwrap()
        .insert("rain_mm_interval".into(), 1.0.into());
    packets[2]
        .as_object_mut()
        .unwrap()
        .insert("soil_moisture_pct".into(), 40.0.into());
    packets[4]
        .as_object_mut()
        .unwrap()
        .insert("rain_mm_interval".into(), 50.0.into());
    packets[4]
        .as_object_mut()
        .unwrap()
        .insert("soil_moisture_pct".into(), 100.0.into());
    packets[4]
        .as_object_mut()
        .unwrap()
        .insert("tilt_pitch_deg".into(), 40.0.into());
    packets[5]
        .as_object_mut()
        .unwrap()
        .insert("tilt_pitch_deg".into(), 30.0.into());
    ingest_packets(&mut store, &packets);

    let rows = build_features(&store).unwrap();
    assert_eq!(rows.len(), 3);
    let at_second = &rows[1];
    assert_eq!(at_second.rain_15m_mm, Some(1.0));
    assert_eq!(at_second.soil_moisture_pct, Some(40.0));
    assert!(at_second.relative_pitch_deg.unwrap() < 10.0);
}

#[test]
fn stale_reference_is_exposed_and_not_used_for_relative_tilt() {
    let tmp = TempDir::new("stale-reference");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    let mut packets = build_stable_dry(1);
    packets[1]
        .as_object_mut()
        .unwrap()
        .insert("ts_device".into(), "2026-11-25T07:58:00+07:00".into());
    ingest_packets(&mut store, &packets);

    let row = &build_features(&store).unwrap()[0];
    assert_eq!(row.reference_age_s, Some(120));
    assert_eq!(row.relative_pitch_deg, None);
    assert!(row.reference_missing);
    assert_eq!(row.stale_count, 1);
}

#[test]
fn label_generation_ignores_current_displacement_and_marks_short_horizon_unknown() {
    let tmp = TempDir::new("labels");
    let features = tmp.path().join("features.csv");
    let events = tmp.path().join("events.csv");
    let truth = tmp.path().join("ground_truth.csv");
    let labels = tmp.path().join("labels.csv");
    let t0 = "2026-11-25T08:00:00+07:00";
    let t1 = "2026-11-25T09:25:00+07:00";
    let t2 = "2026-11-25T10:10:00+07:00";
    std::fs::write(
        &features,
        format!(
            "{FEATURE_HEADER}\n{}\n{}\n{}\n",
            feature_line(t0, "0.0", "40.0", "0.0"),
            feature_line(t1, "0.0", "40.0", "0.0"),
            feature_line(t2, "0.0", "40.0", "0.0")
        ),
    )
    .unwrap();
    std::fs::write(
        &events,
        format!(
            "event_id,start_ts,end_ts,scenario\nE1,{t0},2026-11-25T09:00:00+07:00,controlled\nE2,2026-11-25T09:00:00+07:00,2026-11-25T10:30:00+07:00,stable\n"
        ),
    )
    .unwrap();
    std::fs::write(
        &truth,
        format!(
            "event_id,ts_device,displacement_mm,evidence_file,review_status\nE1,{t0},0.0,camera-e1,reviewed\nE1,2026-11-25T08:10:00+07:00,2.0,camera-e1,reviewed\nE1,2026-11-25T09:00:00+07:00,0.0,camera-e1,reviewed\nE2,{t1},2.0,camera-e2,reviewed\nE2,2026-11-25T09:30:00+07:00,0.0,camera-e2,reviewed\nE2,{t2},0.0,camera-e2,reviewed\n"
        ),
    )
    .unwrap();

    let rows = generate_labels(&features, &events, &truth, 1.0, &labels).unwrap();
    assert_eq!(rows, 3);
    let text = std::fs::read_to_string(labels).unwrap();
    assert!(text.contains("E1,1,LABELED"));
    assert!(text.contains("E2,0,LABELED"));
    assert!(text.contains("INSUFFICIENT_HORIZON"));
}

#[test]
fn malformed_label_row_returns_error() {
    let tmp = TempDir::new("malformed-label");
    let labels = tmp.path().join("labels.csv");
    let split = tmp.path().join("split.csv");
    std::fs::write(
        &labels,
        format!("{FEATURE_HEADER},{LABEL_EXTRA_HEADER}\nshort\n"),
    )
    .unwrap();
    let error = write_split_manifest(&labels, &split)
        .unwrap_err()
        .to_string();
    assert!(error.contains("label row width"));
}

#[test]
fn chronological_split_keeps_each_event_whole() {
    let tmp = TempDir::new("split");
    let labels = tmp.path().join("labels.csv");
    let split = tmp.path().join("split.csv");
    let t0 = "2026-11-25T08:00:00+07:00";
    let t1 = "2026-11-25T09:00:00+07:00";
    let t2 = "2026-11-25T10:00:00+07:00";
    let rows = [
        (t0, "E1", "0", "LABELED"),
        (t1, "E2", "1", "LABELED"),
        (t2, "E3", "0", "LABELED"),
    ];
    std::fs::write(&labels, labels_csv(&rows)).unwrap();
    let count = write_split_manifest(&labels, &split).unwrap();
    assert_eq!(count, 3);
    let text = std::fs::read_to_string(split).unwrap();
    assert!(text.contains("E1,train,"));
    assert!(text.contains("E2,validation,"));
    assert!(text.contains("E3,test,"));
}

#[test]
fn baselines_emit_b0_b1_b2_with_same_split_manifest() {
    let tmp = TempDir::new("baselines");
    let labels = tmp.path().join("labels.csv");
    let split = tmp.path().join("split.csv");
    let results = tmp.path().join("baselines.csv");
    let t0 = "2026-11-25T08:00:00+07:00";
    let t1 = "2026-11-25T09:00:00+07:00";
    let t2 = "2026-11-25T10:00:00+07:00";
    std::fs::write(
        &labels,
        labels_csv(&[
            (t0, "E1", "0", "LABELED"),
            (t1, "E2", "1", "LABELED"),
            (t2, "E3", "0", "LABELED"),
        ]),
    )
    .unwrap();
    write_split_manifest(&labels, &split).unwrap();
    let n = write_baseline_results(&labels, &split, &results, BaselineConfig::default()).unwrap();
    assert_eq!(n, 9);
    let text = std::fs::read_to_string(results).unwrap();
    assert!(text.contains("B0,E1,train"));
    assert!(text.contains("B1,E2,validation"));
    assert!(text.contains("B2,E3,test"));
}

#[test]
fn feature_csv_writer_emits_frozen_header() {
    let tmp = TempDir::new("feature-csv");
    let mut store = Store::open(tmp.path().join("raw.db").to_str().unwrap()).unwrap();
    ingest_packets(&mut store, &build_stable_dry(1));
    let rows = build_features(&store).unwrap();
    let out = tmp.path().join("features.csv");
    write_features_csv(&rows, &out).unwrap();
    let text = std::fs::read_to_string(out).unwrap();
    assert!(text.starts_with(FEATURE_HEADER));
}

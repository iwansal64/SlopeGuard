mod common;

use common::TempDir;
use slopeguard::analysis::{FEATURE_HEADER, LABEL_EXTRA_HEADER};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_slopeguard"))
        .args(args)
        .output()
        .expect("run slopeguard")
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn replay_bytes_match_committed_fixture_then_ingest_is_idempotent() {
    let tmp = TempDir::new("cli");
    let out = tmp.path().join("stable_dry.jsonl");
    let db = tmp.path().join("raw.db");

    let replayed = run(&["replay", "--out", out.to_str().unwrap()]);
    assert!(replayed.status.success(), "{replayed:?}");
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(root().join("data/replays/stable_dry.jsonl")).unwrap()
    );

    let first = run(&[
        "ingest",
        out.to_str().unwrap(),
        "--db",
        db.to_str().unwrap(),
        "--reset",
    ]);
    assert!(first.status.success(), "{first:?}");
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(stdout.contains("accepted=40 rejected=0 duplicates=0 out_of_order=0"));
    assert!(stdout.contains("movement  IMU-MOVE-01  seq=20"));
    assert!(stdout.contains("reference IMU-REF-01   seq=20"));

    let second = run(&[
        "ingest",
        out.to_str().unwrap(),
        "--db",
        db.to_str().unwrap(),
    ]);
    assert_eq!(second.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&second.stdout).contains("duplicates=40"));
}

#[test]
fn models_command_writes_predictions_and_report() {
    let tmp = TempDir::new("models-command");
    let labels = tmp.path().join("labels.csv");
    let split = tmp.path().join("split.csv");
    let baselines = tmp.path().join("baselines.csv");
    let results = tmp.path().join("results.csv");
    let report = tmp.path().join("report.txt");
    let metrics = tmp.path().join("metrics.csv");
    let metrics_report = tmp.path().join("metrics_report.txt");
    let mut text = format!("{FEATURE_HEADER},{LABEL_EXTRA_HEADER}\n");
    for i in 0..10 {
        let event = format!("E{i:02}");
        let hour = 8 + i;
        let (actual, relative, rate, rain, soil) = if i % 2 == 0 {
            (0, "0.1", "0.0", "0.2", "40.0")
        } else {
            (1, "3.0", "0.8", "7.0", "85.0")
        };
        text.push_str(&format!(
            "SLOPE-001,IMU-MOVE-01,2026-11-25T{hour:02}:00:00+07:00,0.0,0.0,{relative},0.0,{rate},1.0,0.0,{rain},0.0,{soil},0.0,0,0,0,false,false,false,{event},{actual},LABELED,rig-marker,1.0,30\n"
        ));
    }
    std::fs::write(&labels, text).unwrap();

    let split_run = run(&[
        "split",
        "--labels",
        labels.to_str().unwrap(),
        "--out",
        split.to_str().unwrap(),
    ]);
    assert!(split_run.status.success(), "{split_run:?}");

    let model_run = run(&[
        "models",
        "--labels",
        labels.to_str().unwrap(),
        "--split",
        split.to_str().unwrap(),
        "--out",
        results.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(model_run.status.success(), "{model_run:?}");
    assert!(String::from_utf8_lossy(&model_run.stdout).contains("model_variants=3 result_rows=30"));
    assert_eq!(
        std::fs::read_to_string(&results).unwrap().lines().count(),
        31
    );
    assert!(
        std::fs::read_to_string(report)
            .unwrap()
            .contains("fit_scope=train_only")
    );

    let baseline_run = run(&[
        "baselines",
        "--labels",
        labels.to_str().unwrap(),
        "--split",
        split.to_str().unwrap(),
        "--out",
        baselines.to_str().unwrap(),
    ]);
    assert!(baseline_run.status.success(), "{baseline_run:?}");

    let metric_run = run(&[
        "metrics",
        "--labels",
        labels.to_str().unwrap(),
        "--split",
        split.to_str().unwrap(),
        "--baselines",
        baselines.to_str().unwrap(),
        "--models",
        results.to_str().unwrap(),
        "--out",
        metrics.to_str().unwrap(),
        "--report",
        metrics_report.to_str().unwrap(),
        "--source-kind",
        "synthetic",
    ]);
    assert!(metric_run.status.success(), "{metric_run:?}");
    assert!(
        String::from_utf8_lossy(&metric_run.stdout).contains("metric_methods=6 metric_rows=18")
    );
    assert_eq!(
        std::fs::read_to_string(metrics).unwrap().lines().count(),
        19
    );
    assert!(
        std::fs::read_to_string(metrics_report)
            .unwrap()
            .contains("claimable=false")
    );
}

#[test]
fn unreadable_file_exits_2() {
    let tmp = TempDir::new("missing");
    let out = run(&[
        "ingest",
        tmp.path().join("nope.jsonl").to_str().unwrap(),
        "--db",
        tmp.path().join("raw.db").to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
}

#[test]
fn features_command_writes_causal_csv() {
    let tmp = TempDir::new("features-command");
    let replay = tmp.path().join("replay.jsonl");
    let db = tmp.path().join("raw.db");
    let features = tmp.path().join("features.csv");
    let replayed = run(&["replay", "--out", replay.to_str().unwrap(), "--n", "3"]);
    assert!(replayed.status.success(), "{replayed:?}");
    let ingested = run(&[
        "ingest",
        replay.to_str().unwrap(),
        "--db",
        db.to_str().unwrap(),
        "--reset",
    ]);
    assert!(ingested.status.success(), "{ingested:?}");
    let built = run(&[
        "features",
        "--db",
        db.to_str().unwrap(),
        "--out",
        features.to_str().unwrap(),
    ]);
    assert!(built.status.success(), "{built:?}");
    let text = std::fs::read_to_string(features).unwrap();
    assert!(text.starts_with("site_id,node_id,ts_device"));
    assert_eq!(text.lines().count(), 4);
}

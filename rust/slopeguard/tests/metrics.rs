mod common;

use common::TempDir;
use slopeguard::analysis::{
    FEATURE_HEADER, LABEL_EXTRA_HEADER, write_baseline_results, write_split_manifest,
};
use slopeguard::metrics::{METRIC_HEADER, run_metrics};
use slopeguard::model::run_models;

fn labels_csv() -> String {
    let mut out = format!("{FEATURE_HEADER},{LABEL_EXTRA_HEADER}\n");
    for i in 0..10 {
        let event = format!("E{i:02}");
        let hour = 8 + i;
        let (actual, relative, rate, rain, soil) = if i % 2 == 0 {
            (0, "0.1", "0.0", "0.2", "40.0")
        } else {
            (1, "3.0", "0.8", "7.0", "85.0")
        };
        out.push_str(&format!(
            "SLOPE-001,IMU-MOVE-01,2026-11-25T{hour:02}:00:00+07:00,0.0,0.0,{relative},0.0,{rate},1.0,0.0,{rain},0.0,{soil},0.0,0,0,0,false,false,false,{event},{actual},LABELED,rig-marker,1.0,30\n"
        ));
    }
    out
}

#[test]
fn metrics_emit_event_and_row_results_with_non_reportable_status() {
    let tmp = TempDir::new("metrics");
    let labels = tmp.path().join("labels.csv");
    let split = tmp.path().join("split.csv");
    let baselines = tmp.path().join("baselines.csv");
    let model_results = tmp.path().join("model_results.csv");
    let model_report = tmp.path().join("model_report.txt");
    let metrics = tmp.path().join("metrics.csv");
    let report = tmp.path().join("metrics_report.txt");
    std::fs::write(&labels, labels_csv()).unwrap();
    write_split_manifest(&labels, &split).unwrap();
    write_baseline_results(&labels, &split, &baselines, Default::default()).unwrap();
    run_models(&labels, &split, &model_results, &model_report).unwrap();

    let summary = run_metrics(
        &labels,
        &split,
        &baselines,
        &model_results,
        &metrics,
        &report,
        "synthetic",
    )
    .unwrap();

    assert_eq!(summary.methods, 6);
    assert_eq!(summary.rows, 18);
    let metric_text = std::fs::read_to_string(metrics).unwrap();
    assert!(metric_text.starts_with(METRIC_HEADER));
    assert_eq!(metric_text.lines().count(), 19);
    assert!(metric_text.contains("B0,test,"));
    assert!(metric_text.contains("fused,test,"));

    let report_text = std::fs::read_to_string(report).unwrap();
    assert!(report_text.contains("source_kind=synthetic"));
    assert!(report_text.contains("claimable=false"));
    assert!(report_text.contains("NOT_REPORTABLE_UNTIL_REAL_CONTROLLED_RIG"));
    assert!(report_text.contains("method=B0 split=test"));
    assert!(report_text.contains("method=fused split=test"));
}

#[test]
fn metrics_are_byte_deterministic() {
    let first = TempDir::new("metrics-a");
    let second = TempDir::new("metrics-b");
    let build = |tmp: &TempDir| {
        let labels = tmp.path().join("labels.csv");
        let split = tmp.path().join("split.csv");
        let baselines = tmp.path().join("baselines.csv");
        let model_results = tmp.path().join("model_results.csv");
        let model_report = tmp.path().join("model_report.txt");
        let metrics = tmp.path().join("metrics.csv");
        let report = tmp.path().join("metrics_report.txt");
        std::fs::write(&labels, labels_csv()).unwrap();
        write_split_manifest(&labels, &split).unwrap();
        write_baseline_results(&labels, &split, &baselines, Default::default()).unwrap();
        run_models(&labels, &split, &model_results, &model_report).unwrap();
        run_metrics(
            &labels,
            &split,
            &baselines,
            &model_results,
            &metrics,
            &report,
            "synthetic",
        )
        .unwrap();
        (metrics, report)
    };
    let (metrics_a, report_a) = build(&first);
    let (metrics_b, report_b) = build(&second);
    assert_eq!(
        std::fs::read(metrics_a).unwrap(),
        std::fs::read(metrics_b).unwrap()
    );
    assert_eq!(
        std::fs::read(report_a).unwrap(),
        std::fs::read(report_b).unwrap()
    );
}

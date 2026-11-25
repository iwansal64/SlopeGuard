mod common;

use common::TempDir;
use slopeguard::analysis::{FEATURE_HEADER, LABEL_EXTRA_HEADER, write_split_manifest};
use slopeguard::model::{MODEL_RESULT_HEADER, run_models};

fn label_row(
    event: &str,
    ts: &str,
    actual: i32,
    relative_pitch: &str,
    tilt_rate: &str,
    rain_15: &str,
    soil: &str,
) -> String {
    format!(
        "SLOPE-001,IMU-MOVE-01,{ts},0.0,0.0,{relative_pitch},0.0,{tilt_rate},1.0,0.0,{rain_15},0.0,{soil},0.0,0,0,0,false,false,false,{event},{actual},LABELED,rig-marker,1.0,30"
    )
}

fn labels_csv(test_labels: &[i32]) -> String {
    let mut out = format!("{FEATURE_HEADER},{LABEL_EXTRA_HEADER}\n");
    let rows = [
        (
            "E01",
            "2026-11-25T08:00:00+07:00",
            0,
            "0.0",
            "0.0",
            "0.0",
            "40.0",
        ),
        (
            "E02",
            "2026-11-25T09:00:00+07:00",
            1,
            "4.0",
            "1.0",
            "8.0",
            "90.0",
        ),
        (
            "E03",
            "2026-11-25T10:00:00+07:00",
            0,
            "0.5",
            "0.0",
            "1.0",
            "45.0",
        ),
        (
            "E04",
            "2026-11-25T11:00:00+07:00",
            1,
            "3.5",
            "0.8",
            "7.0",
            "88.0",
        ),
        (
            "E05",
            "2026-11-25T12:00:00+07:00",
            0,
            "0.2",
            "0.0",
            "0.5",
            "42.0",
        ),
        (
            "E06",
            "2026-11-25T13:00:00+07:00",
            1,
            "3.0",
            "0.7",
            "6.0",
            "85.0",
        ),
        (
            "E07",
            "2026-11-25T14:00:00+07:00",
            1,
            "2.8",
            "0.6",
            "5.0",
            "80.0",
        ),
        (
            "E08",
            "2026-11-25T15:00:00+07:00",
            0,
            "0.1",
            "0.0",
            "0.2",
            "41.0",
        ),
        (
            "E09",
            "2026-11-25T16:00:00+07:00",
            test_labels[0],
            "3.2",
            "0.9",
            "7.5",
            "87.0",
        ),
        (
            "E10",
            "2026-11-25T17:00:00+07:00",
            test_labels[1],
            "0.0",
            "0.0",
            "0.0",
            "39.0",
        ),
    ];
    for (event, ts, label, relative_pitch, tilt_rate, rain_15, soil) in rows {
        out.push_str(&label_row(
            event,
            ts,
            label,
            relative_pitch,
            tilt_rate,
            rain_15,
            soil,
        ));
        out.push('\n');
    }
    out
}

fn prepare(tmp: &TempDir, test_labels: &[i32]) -> (std::path::PathBuf, std::path::PathBuf) {
    let labels = tmp.path().join("labels.csv");
    let split = tmp.path().join("split.csv");
    std::fs::write(&labels, labels_csv(test_labels)).unwrap();
    write_split_manifest(&labels, &split).unwrap();
    (labels, split)
}

#[test]
fn model_run_emits_three_ablations_and_all_split_predictions() {
    let tmp = TempDir::new("model-run");
    let (labels, split) = prepare(&tmp, &[1, 0, 0, 1]);
    let results = tmp.path().join("results.csv");
    let report = tmp.path().join("report.txt");

    let summary = run_models(&labels, &split, &results, &report).unwrap();

    assert_eq!(summary.variants, 3);
    assert_eq!(summary.result_rows, 30);
    let result_text = std::fs::read_to_string(results).unwrap();
    assert!(result_text.starts_with(MODEL_RESULT_HEADER));
    assert_eq!(result_text.lines().count(), 31);
    assert!(result_text.contains("motion,E01,train,"));
    assert!(result_text.contains("environment,E07,validation,"));
    assert!(result_text.contains("fused,E10,test,"));

    let report_text = std::fs::read_to_string(report).unwrap();
    assert!(report_text.contains("variant=motion"));
    assert!(report_text.contains("variant=environment"));
    assert!(report_text.contains("variant=fused"));
    assert!(report_text.contains("threshold_source=validation"));
    assert!(report_text.contains("split=train"));
    assert!(report_text.contains("split=validation"));
    assert!(report_text.contains("split=test"));
}

#[test]
fn model_run_is_deterministic_and_test_labels_cannot_change_thresholds() {
    let first = TempDir::new("model-deterministic-a");
    let second = TempDir::new("model-deterministic-b");
    let (labels_a, split_a) = prepare(&first, &[1, 0, 0, 1]);
    let (labels_b, split_b) = prepare(&second, &[0, 1, 1, 0]);
    let results_a = first.path().join("results.csv");
    let report_a = first.path().join("report.txt");
    let results_b = second.path().join("results.csv");
    let report_b = second.path().join("report.txt");

    run_models(&labels_a, &split_a, &results_a, &report_a).unwrap();
    run_models(&labels_b, &split_b, &results_b, &report_b).unwrap();

    let report_a = std::fs::read_to_string(report_a).unwrap();
    let report_b = std::fs::read_to_string(report_b).unwrap();
    let thresholds = |report: &str| {
        report
            .lines()
            .filter(|line| line.starts_with("variant=") && line.contains("threshold="))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(thresholds(&report_a), thresholds(&report_b));

    let results_a = std::fs::read_to_string(results_a).unwrap();
    let results_b = std::fs::read_to_string(results_b).unwrap();
    assert_ne!(results_a, results_b);
}

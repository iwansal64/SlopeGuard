//! Deterministic event- and row-level evaluation over frozen predictions.
//!
//! Metrics are evaluation artifacts, not automatic claims. Synthetic input is
//! explicitly marked non-reportable; real controlled-rig output remains review
//! required until provenance and label gates are checked by the team.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::analysis::{BASELINE_HEADER, FEATURE_HEADER, LABEL_EXTRA_HEADER};
use crate::model::MODEL_RESULT_HEADER;

pub const METRIC_HEADER: &str = "method,split,row_count,event_count,positive_events,detected_events,missed_events,false_alert_events,event_recall,event_false_alert_rate,row_accuracy,row_precision,row_recall,f1,score_mse,claimable,source_kind";

const METHODS: &[(&str, &str)] = &[
    ("B0", "baseline"),
    ("B1", "baseline"),
    ("B2", "baseline"),
    ("motion", "model"),
    ("environment", "model"),
    ("fused", "model"),
];
const SPLITS: &[&str] = &["train", "validation", "test"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsSummary {
    pub methods: usize,
    pub rows: usize,
}

#[derive(Debug, Clone)]
struct Prediction {
    method: String,
    event_id: String,
    split: String,
    actual: i32,
    predicted: i32,
    score: f64,
}

#[derive(Debug, Default, Clone, Copy)]
struct EventOutcome {
    actual_positive: bool,
    predicted_positive: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct Confusion {
    true_positive: usize,
    false_positive: usize,
    true_negative: usize,
    false_negative: usize,
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field)),
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields
}

fn read_csv(path: &Path) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(split_csv_line)
        .collect())
}

fn column(header: &[String], name: &str) -> Result<usize, Box<dyn std::error::Error>> {
    header
        .iter()
        .position(|value| value == name)
        .ok_or_else(|| format!("missing column {name}").into())
}

fn parse_binary(value: &str, name: &str) -> Result<i32, Box<dyn std::error::Error>> {
    let parsed: i32 = value.parse()?;
    if matches!(parsed, 0 | 1) {
        Ok(parsed)
    } else {
        Err(format!("{name} must be 0 or 1").into())
    }
}

fn parse_splits(path: &Path) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let rows = read_csv(path)?;
    let header = rows.first().ok_or("empty split manifest")?;
    if header != &split_csv_line("event_id,split,start_ts,end_ts,row_count") {
        return Err("split header does not match SPLIT_HEADER".into());
    }
    let mut result = HashMap::new();
    for row in rows.into_iter().skip(1) {
        if row.len() != 5 {
            return Err("split row width does not match split header".into());
        }
        if !SPLITS.contains(&row[1].as_str()) {
            return Err(format!("unknown split {}", row[1]).into());
        }
        if result.insert(row[0].clone(), row[1].clone()).is_some() {
            return Err(format!("duplicate split event {}", row[0]).into());
        }
    }
    if result.is_empty() {
        return Err("split manifest has no events".into());
    }
    Ok(result)
}

fn parse_labels(path: &Path) -> Result<HashMap<(String, String), i32>, Box<dyn std::error::Error>> {
    let rows = read_csv(path)?;
    let header = rows.first().ok_or("empty labels csv")?.clone();
    let expected = split_csv_line(&format!("{FEATURE_HEADER},{LABEL_EXTRA_HEADER}"));
    if header != expected {
        return Err("labels header does not match feature and label contract".into());
    }
    let event_idx = column(&header, "event_id")?;
    let ts_idx = column(&header, "ts_device")?;
    let actual_idx = column(&header, "movement_event")?;
    let status_idx = column(&header, "label_status")?;
    let mut labels = HashMap::new();
    for row in rows.into_iter().skip(1) {
        if row.len() != header.len() {
            return Err("label row width does not match labels header".into());
        }
        if row[status_idx] != "LABELED" {
            continue;
        }
        let key = (row[event_idx].clone(), row[ts_idx].clone());
        let actual = parse_binary(&row[actual_idx], "movement_event")?;
        if labels.insert(key.clone(), actual).is_some() {
            return Err(format!("duplicate labeled row {key:?}").into());
        }
    }
    if labels.is_empty() {
        return Err("no LABELED rows in labels csv".into());
    }
    Ok(labels)
}

fn parse_predictions(
    path: &Path,
    kind: &str,
    labels: &HashMap<(String, String), i32>,
    splits: &HashMap<String, String>,
) -> Result<Vec<Prediction>, Box<dyn std::error::Error>> {
    let rows = read_csv(path)?;
    let header = rows.first().ok_or("empty prediction csv")?.clone();
    let expected = match kind {
        "baseline" => split_csv_line(BASELINE_HEADER),
        "model" => split_csv_line(MODEL_RESULT_HEADER),
        _ => return Err(format!("unknown prediction kind {kind}").into()),
    };
    if header != expected {
        return Err(format!("{kind} header does not match its contract").into());
    }
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for row in rows.into_iter().skip(1) {
        if row.len() != header.len() {
            return Err(format!("{kind} row width does not match its header").into());
        }
        let (method, event_idx, split_idx, ts_idx, actual_idx, score_idx, predicted_idx) =
            if kind == "baseline" {
                (
                    row[0].clone(),
                    1usize,
                    2usize,
                    3usize,
                    4usize,
                    6usize,
                    5usize,
                )
            } else {
                (
                    row[0].clone(),
                    1usize,
                    2usize,
                    3usize,
                    4usize,
                    5usize,
                    6usize,
                )
            };
        if !METHODS
            .iter()
            .any(|(candidate, candidate_kind)| *candidate == method && *candidate_kind == kind)
        {
            return Err(format!("unexpected {kind} method {method}").into());
        }
        let event_id = row[event_idx].clone();
        let split = row[split_idx].clone();
        if !SPLITS.contains(&split.as_str()) {
            return Err(format!("unknown prediction split {split}").into());
        }
        if splits.get(&event_id) != Some(&split) {
            return Err(format!("prediction split disagrees for event {event_id}").into());
        }
        let ts = row[ts_idx].clone();
        let key = (event_id.clone(), ts.clone());
        let Some(expected_actual) = labels.get(&key) else {
            return Err(format!("prediction has no labeled row {key:?}").into());
        };
        let actual = parse_binary(&row[actual_idx], "actual")?;
        if actual != *expected_actual {
            return Err(format!("prediction actual disagrees for {key:?}").into());
        }
        let predicted = parse_binary(&row[predicted_idx], "predicted")?;
        let score: f64 = row[score_idx].parse()?;
        if !score.is_finite() {
            return Err("prediction score must be finite".into());
        }
        if !seen.insert((method.clone(), split.clone(), key.clone())) {
            return Err(format!("duplicate prediction {method}/{split}/{key:?}").into());
        }
        result.push(Prediction {
            method,
            event_id,
            split,
            actual,
            predicted,
            score,
        });
    }
    Ok(result)
}

fn rates(confusion: Confusion) -> (f64, f64, f64, f64, f64) {
    let tp = confusion.true_positive as f64;
    let fp = confusion.false_positive as f64;
    let tn = confusion.true_negative as f64;
    let fn_ = confusion.false_negative as f64;
    let total = tp + fp + tn + fn_;
    let precision = if tp + fp > 0.0 { tp / (tp + fp) } else { 0.0 };
    let recall = if tp + fn_ > 0.0 { tp / (tp + fn_) } else { 0.0 };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    let accuracy = if total > 0.0 { (tp + tn) / total } else { 0.0 };
    (accuracy, precision, recall, f1, total)
}

fn claim_status(source_kind: &str) -> (&'static str, &'static str) {
    if source_kind == "real_controlled_rig" {
        ("review_required", "REVIEW_REQUIRED_REAL_CONTROLLED_RIG")
    } else {
        ("false", "NOT_REPORTABLE_UNTIL_REAL_CONTROLLED_RIG")
    }
}

fn metric_row(
    method: &str,
    split: &str,
    rows: &[Prediction],
    claimable: &str,
    source_kind: &str,
) -> String {
    let mut confusion = Confusion::default();
    let mut score_mse = 0.0;
    let mut events: BTreeMap<&str, EventOutcome> = BTreeMap::new();
    for row in rows {
        match (row.actual, row.predicted) {
            (1, 1) => confusion.true_positive += 1,
            (0, 1) => confusion.false_positive += 1,
            (0, 0) => confusion.true_negative += 1,
            (1, 0) => confusion.false_negative += 1,
            _ => unreachable!("binary predictions validated before metrics"),
        }
        let error = row.score - f64::from(row.actual);
        score_mse += error * error;
        let outcome = events.entry(row.event_id.as_str()).or_default();
        outcome.actual_positive |= row.actual == 1;
        outcome.predicted_positive |= row.predicted == 1;
    }
    let event_count = events.len();
    let positive_events = events
        .values()
        .filter(|event| event.actual_positive)
        .count();
    let detected_events = events
        .values()
        .filter(|event| event.actual_positive && event.predicted_positive)
        .count();
    let missed_events = positive_events - detected_events;
    let false_alert_events = events
        .values()
        .filter(|event| !event.actual_positive && event.predicted_positive)
        .count();
    let negative_events = event_count - positive_events;
    let event_recall = if positive_events > 0 {
        detected_events as f64 / positive_events as f64
    } else {
        0.0
    };
    let event_false_alert_rate = if negative_events > 0 {
        false_alert_events as f64 / negative_events as f64
    } else {
        0.0
    };
    let (row_accuracy, row_precision, row_recall, f1, row_total) = rates(confusion);
    let score_mse = if row_total > 0.0 {
        score_mse / row_total
    } else {
        0.0
    };
    format!(
        "{method},{split},{},{event_count},{positive_events},{detected_events},{missed_events},{false_alert_events},{event_recall:.6},{event_false_alert_rate:.6},{row_accuracy:.6},{row_precision:.6},{row_recall:.6},{f1:.6},{score_mse:.6},{claimable},{source_kind}",
        rows.len()
    )
}

/// Evaluate B0/B1/B2 and motion/environment/fused predictions without claiming
/// synthetic performance as field validation.
pub fn run_metrics(
    labels_path: &Path,
    split_path: &Path,
    baselines_path: &Path,
    model_results_path: &Path,
    metrics_path: &Path,
    report_path: &Path,
    source_kind: &str,
) -> Result<MetricsSummary, Box<dyn std::error::Error>> {
    if source_kind.trim().is_empty() || source_kind.contains([',', '\n', '\r']) {
        return Err("source_kind must be non-empty and CSV-safe".into());
    }
    let labels = parse_labels(labels_path)?;
    let splits = parse_splits(split_path)?;
    let mut predictions = parse_predictions(baselines_path, "baseline", &labels, &splits)?;
    predictions.extend(parse_predictions(
        model_results_path,
        "model",
        &labels,
        &splits,
    )?);
    let mut grouped: BTreeMap<(String, String), Vec<Prediction>> = BTreeMap::new();
    for prediction in predictions {
        grouped
            .entry((prediction.method.clone(), prediction.split.clone()))
            .or_default()
            .push(prediction);
    }
    let (claimable, status) = claim_status(source_kind);
    let mut metrics = String::new();
    writeln!(metrics, "{METRIC_HEADER}").expect("writing String cannot fail");
    let mut report = String::new();
    writeln!(report, "SLOPEGUARD METRICS REPORT v1").expect("writing String cannot fail");
    writeln!(report, "source_kind={source_kind}").expect("writing String cannot fail");
    writeln!(report, "claimable={claimable}").expect("writing String cannot fail");
    writeln!(report, "status={status}").expect("writing String cannot fail");
    writeln!(report, "labels=independent_labeled_rows_from_labels_csv")
        .expect("writing String cannot fail");
    writeln!(report, "split_policy=event_wise_chronological_manifest")
        .expect("writing String cannot fail");
    writeln!(
        report,
        "headline_metric=event_recall_and_event_false_alert_rate"
    )
    .expect("writing String cannot fail");
    writeln!(report, "row_accuracy_is_diagnostic_only=true").expect("writing String cannot fail");
    writeln!(
        report,
        "score_mse_is_not_calibration_for_threshold_baselines=true"
    )
    .expect("writing String cannot fail");

    let mut rows_written = 0;
    for (method, _) in METHODS {
        for split in SPLITS {
            let Some(rows) = grouped.get(&(method.to_string(), (*split).to_string())) else {
                return Err(format!("missing predictions for {method}/{split}").into());
            };
            if rows.is_empty() {
                return Err(format!("empty predictions for {method}/{split}").into());
            }
            writeln!(
                metrics,
                "{}",
                metric_row(method, split, rows, claimable, source_kind)
            )
            .expect("writing String cannot fail");
            writeln!(
                report,
                "method={method} split={split} {}",
                metric_row(method, split, rows, claimable, source_kind)
            )
            .expect("writing String cannot fail");
            rows_written += 1;
        }
    }
    if let Some(parent) = metrics_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = report_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(metrics_path, metrics)?;
    std::fs::write(report_path, report)?;
    Ok(MetricsSummary {
        methods: METHODS.len(),
        rows: rows_written,
    })
}

#[cfg(test)]
mod tests {
    use super::rates;

    #[test]
    fn rates_zero_division_is_deterministic() {
        let (accuracy, precision, recall, f1, total) = rates(Default::default());
        assert_eq!(
            (accuracy, precision, recall, f1, total),
            (0.0, 0.0, 0.0, 0.0, 0.0)
        );
    }
}

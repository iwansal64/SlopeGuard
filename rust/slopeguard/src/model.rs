//! Deterministic, leakage-safe logistic regression over the frozen label CSV.
//!
//! Fitting uses train events only. The decision threshold is selected on
//! validation events with F2, then test events are evaluated once.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use chrono::{DateTime, FixedOffset};

use crate::analysis::{FEATURE_HEADER, LABEL_EXTRA_HEADER};

pub const MODEL_RESULT_HEADER: &str =
    "variant,event_id,split,ts_device,actual,score,predicted,threshold,feature_count";

const MOTION_FEATURES: &[&str] = &[
    "relative_pitch_deg",
    "relative_roll_deg",
    "tilt_rate_deg_min",
    "vibration_mag_g",
    "accel_mag_g",
];
const ENVIRONMENT_FEATURES: &[&str] = &[
    "rain_15m_mm",
    "rain_30m_mm",
    "soil_moisture_pct",
    "soil_rate_pct_min",
];
const FUSED_FEATURES: &[&str] = &[
    "relative_pitch_deg",
    "relative_roll_deg",
    "tilt_rate_deg_min",
    "vibration_mag_g",
    "accel_mag_g",
    "rain_15m_mm",
    "rain_30m_mm",
    "soil_moisture_pct",
    "soil_rate_pct_min",
];
const VARIANTS: &[(&str, &[&str])] = &[
    ("motion", MOTION_FEATURES),
    ("environment", ENVIRONMENT_FEATURES),
    ("fused", FUSED_FEATURES),
];
const ALL_FEATURES: &[&str] = FUSED_FEATURES;
const L2_LAMBDA: f64 = 0.01;
const LEARNING_RATE: f64 = 0.10;
const MAX_EPOCHS: usize = 2_000;
const GRADIENT_TOLERANCE: f64 = 1e-8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSummary {
    pub variants: usize,
    pub result_rows: usize,
}

#[derive(Debug, Clone)]
struct LabelRow {
    event_id: String,
    ts: String,
    sort_ts: DateTime<FixedOffset>,
    actual: i32,
    features: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
struct Scaler {
    means: Vec<f64>,
    scales: Vec<f64>,
}

#[derive(Debug, Clone)]
struct LogisticModel {
    weights: Vec<f64>,
    bias: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct Confusion {
    true_positive: usize,
    false_positive: usize,
    true_negative: usize,
    false_negative: usize,
}

#[derive(Debug, Clone, Copy)]
struct Metrics {
    confusion: Confusion,
    accuracy: f64,
    precision: f64,
    recall: f64,
    f1: f64,
    f2: f64,
    specificity: f64,
    balanced_accuracy: f64,
    brier: f64,
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

fn parse_ts(value: &str) -> Result<DateTime<FixedOffset>, Box<dyn std::error::Error>> {
    Ok(DateTime::parse_from_rfc3339(value)?)
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn parse_label_rows(path: &Path) -> Result<Vec<LabelRow>, Box<dyn std::error::Error>> {
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
    let feature_indices = ALL_FEATURES
        .iter()
        .map(|name| column(&header, name))
        .collect::<Result<Vec<_>, _>>()?;

    let mut parsed = Vec::new();
    for row in rows.into_iter().skip(1) {
        if row.len() != header.len() {
            return Err("label row width does not match labels header".into());
        }
        if row[status_idx] != "LABELED" {
            continue;
        }
        let actual: i32 = row[actual_idx].parse()?;
        if !matches!(actual, 0 | 1) {
            return Err("labeled movement_event must be 0 or 1".into());
        }
        let ts = parse_ts(&row[ts_idx])?;
        let features = feature_indices
            .iter()
            .map(|index| {
                let value = &row[*index];
                if value.is_empty() {
                    Ok(None)
                } else {
                    let parsed: f64 = value.parse()?;
                    if !parsed.is_finite() {
                        return Err("feature value must be finite".into());
                    }
                    Ok(Some(parsed))
                }
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        parsed.push(LabelRow {
            event_id: row[event_idx].clone(),
            ts: row[ts_idx].clone(),
            sort_ts: ts,
            actual,
            features,
        });
    }
    if parsed.is_empty() {
        return Err("no LABELED rows in labels csv".into());
    }
    parsed.sort_by(|left, right| {
        left.sort_ts
            .cmp(&right.sort_ts)
            .then_with(|| left.event_id.cmp(&right.event_id))
            .then_with(|| left.ts.cmp(&right.ts))
    });
    Ok(parsed)
}

fn parse_splits(path: &Path) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let rows = read_csv(path)?;
    let header = rows.first().ok_or("empty split manifest")?;
    if header != &split_csv_line("event_id,split,start_ts,end_ts,row_count") {
        return Err("split header does not match SPLIT_HEADER".into());
    }
    let mut splits = HashMap::new();
    for row in rows.into_iter().skip(1) {
        if row.len() != 5 {
            return Err("split row width does not match split header".into());
        }
        if !matches!(row[1].as_str(), "train" | "validation" | "test") {
            return Err(format!("unknown split {}", row[1]).into());
        }
        if splits.insert(row[0].clone(), row[1].clone()).is_some() {
            return Err(format!("duplicate split event {}", row[0]).into());
        }
    }
    if splits.is_empty() {
        return Err("split manifest has no events".into());
    }
    Ok(splits)
}

fn feature_indices(names: &[&str]) -> Vec<usize> {
    names
        .iter()
        .map(|name| {
            ALL_FEATURES
                .iter()
                .position(|candidate| candidate == name)
                .expect("variant feature must be in ALL_FEATURES")
        })
        .collect()
}

fn rows_for_split<'a>(
    rows: &'a [LabelRow],
    splits: &HashMap<String, String>,
    split: &str,
) -> Result<Vec<&'a LabelRow>, Box<dyn std::error::Error>> {
    rows.iter()
        .filter(|row| {
            splits
                .get(&row.event_id)
                .is_some_and(|assigned| assigned == split)
        })
        .map(|row| {
            if !splits.contains_key(&row.event_id) {
                return Err(format!("missing split for {}", row.event_id).into());
            }
            Ok(row)
        })
        .collect()
}

fn fit_scaler(rows: &[&LabelRow], indices: &[usize]) -> Scaler {
    let means = indices
        .iter()
        .map(|index| {
            let values = rows.iter().filter_map(|row| row.features[*index]);
            let (sum, count) =
                values.fold((0.0, 0_u32), |(sum, count), value| (sum + value, count + 1));
            if count == 0 {
                0.0
            } else {
                sum / f64::from(count)
            }
        })
        .collect::<Vec<_>>();
    let scales = indices
        .iter()
        .enumerate()
        .map(|(position, index)| {
            let mut sum = 0.0;
            let mut count = 0_u32;
            for row in rows {
                if let Some(value) = row.features[*index] {
                    let delta = value - means[position];
                    sum += delta * delta;
                    count += 1;
                }
            }
            let scale = if count == 0 {
                1.0
            } else {
                (sum / f64::from(count)).sqrt()
            };
            if scale.is_finite() && scale > 1e-12 {
                scale
            } else {
                1.0
            }
        })
        .collect();
    Scaler { means, scales }
}

fn vector(row: &LabelRow, indices: &[usize], scaler: &Scaler) -> Vec<f64> {
    indices
        .iter()
        .enumerate()
        .map(|(position, index)| {
            let value = row.features[*index].unwrap_or(scaler.means[position]);
            (value - scaler.means[position]) / scaler.scales[position]
        })
        .collect()
}

fn sigmoid(value: f64) -> f64 {
    if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let exp = value.exp();
        exp / (1.0 + exp)
    }
}

impl LogisticModel {
    fn fit(
        rows: &[&LabelRow],
        indices: &[usize],
        scaler: &Scaler,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let positive = rows.iter().filter(|row| row.actual == 1).count();
        let negative = rows.len() - positive;
        if positive == 0 || negative == 0 {
            return Err("train split must contain both movement classes".into());
        }
        let mut model = Self {
            weights: vec![0.0; indices.len()],
            bias: 0.0,
        };
        let n = rows.len() as f64;
        for _ in 0..MAX_EPOCHS {
            let mut gradients = vec![0.0; indices.len()];
            let mut bias_gradient = 0.0;
            for row in rows {
                let values = vector(row, indices, scaler);
                let logit = model.bias
                    + model
                        .weights
                        .iter()
                        .zip(&values)
                        .map(|(weight, value)| weight * value)
                        .sum::<f64>();
                let error = sigmoid(logit) - f64::from(row.actual);
                bias_gradient += error;
                for (gradient, value) in gradients.iter_mut().zip(values) {
                    *gradient += error * value;
                }
            }
            bias_gradient /= n;
            let mut max_gradient = bias_gradient.abs();
            for (gradient, weight) in gradients.iter_mut().zip(&model.weights) {
                *gradient = *gradient / n + L2_LAMBDA * weight;
                max_gradient = max_gradient.max(gradient.abs());
            }
            model.bias -= LEARNING_RATE * bias_gradient;
            for (weight, gradient) in model.weights.iter_mut().zip(gradients) {
                *weight -= LEARNING_RATE * gradient;
            }
            if max_gradient < GRADIENT_TOLERANCE {
                break;
            }
        }
        Ok(model)
    }

    fn score(&self, row: &LabelRow, indices: &[usize], scaler: &Scaler) -> f64 {
        let values = vector(row, indices, scaler);
        sigmoid(
            self.bias
                + self
                    .weights
                    .iter()
                    .zip(values)
                    .map(|(weight, value)| weight * value)
                    .sum::<f64>(),
        )
    }
}

fn metrics(rows: &[&LabelRow], scores: &[f64], threshold: f64) -> Metrics {
    let mut confusion = Confusion::default();
    let mut brier = 0.0;
    for (row, score) in rows.iter().zip(scores) {
        let predicted = i32::from(*score >= threshold);
        match (row.actual, predicted) {
            (1, 1) => confusion.true_positive += 1,
            (0, 1) => confusion.false_positive += 1,
            (0, 0) => confusion.true_negative += 1,
            (1, 0) => confusion.false_negative += 1,
            _ => unreachable!("labels are validated as binary"),
        }
        let error = *score - f64::from(row.actual);
        brier += error * error;
    }
    let tp = confusion.true_positive as f64;
    let fp = confusion.false_positive as f64;
    let tn = confusion.true_negative as f64;
    let fn_ = confusion.false_negative as f64;
    let n = rows.len() as f64;
    let precision = if tp + fp > 0.0 { tp / (tp + fp) } else { 0.0 };
    let recall = if tp + fn_ > 0.0 { tp / (tp + fn_) } else { 0.0 };
    let specificity = if tn + fp > 0.0 { tn / (tn + fp) } else { 0.0 };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    let beta_squared = 4.0;
    let f2 = if beta_squared * precision + recall > 0.0 {
        (1.0 + beta_squared) * precision * recall / (beta_squared * precision + recall)
    } else {
        0.0
    };
    Metrics {
        confusion,
        accuracy: if n > 0.0 { (tp + tn) / n } else { 0.0 },
        precision,
        recall,
        f1,
        f2,
        specificity,
        balanced_accuracy: (recall + specificity) / 2.0,
        brier: if n > 0.0 { brier / n } else { 0.0 },
    }
}

fn select_threshold(rows: &[&LabelRow], scores: &[f64]) -> f64 {
    let mut best_threshold = 0.5;
    let mut best = metrics(rows, scores, best_threshold);
    for step in 0..=100 {
        let threshold = f64::from(step) / 100.0;
        let candidate = metrics(rows, scores, threshold);
        let better = candidate.f2 > best.f2 + 1e-12
            || ((candidate.f2 - best.f2).abs() <= 1e-12
                && (candidate.recall > best.recall + 1e-12
                    || ((candidate.recall - best.recall).abs() <= 1e-12
                        && threshold < best_threshold)));
        if better {
            best_threshold = threshold;
            best = candidate;
        }
    }
    best_threshold
}

fn metric_line(variant: &str, split: &str, metrics: Metrics, threshold: f64, n: usize) -> String {
    format!(
        "metric variant={variant} split={split} n={n} positives={} threshold={threshold:.2} accuracy={:.6} precision={:.6} recall={:.6} f1={:.6} f2={:.6} specificity={:.6} balanced_accuracy={:.6} brier={:.6} tp={} fp={} tn={} fn={}",
        metrics.confusion.true_positive + metrics.confusion.false_negative,
        metrics.accuracy,
        metrics.precision,
        metrics.recall,
        metrics.f1,
        metrics.f2,
        metrics.specificity,
        metrics.balanced_accuracy,
        metrics.brier,
        metrics.confusion.true_positive,
        metrics.confusion.false_positive,
        metrics.confusion.true_negative,
        metrics.confusion.false_negative,
    )
}

fn write_model_outputs(
    rows: &[LabelRow],
    splits: &HashMap<String, String>,
    results_path: &Path,
    report_path: &Path,
) -> Result<ModelSummary, Box<dyn std::error::Error>> {
    let train = rows_for_split(rows, splits, "train")?;
    let validation = rows_for_split(rows, splits, "validation")?;
    let test = rows_for_split(rows, splits, "test")?;
    if train.is_empty() || validation.is_empty() || test.is_empty() {
        return Err("train, validation, and test must each contain labeled rows".into());
    }

    let mut results = String::new();
    writeln!(results, "{MODEL_RESULT_HEADER}").expect("writing String cannot fail");
    let mut report = String::new();
    writeln!(report, "SLOPEGUARD MODEL REPORT v1").expect("writing String cannot fail");
    writeln!(report, "algorithm=logistic_regression").expect("writing String cannot fail");
    writeln!(report, "target=movement_event").expect("writing String cannot fail");
    writeln!(report, "horizon_minutes=30").expect("writing String cannot fail");
    writeln!(report, "split_policy=event_wise_chronological_manifest")
        .expect("writing String cannot fail");
    writeln!(report, "fit_scope=train_only").expect("writing String cannot fail");
    writeln!(report, "threshold_metric=F2_beta_2").expect("writing String cannot fail");
    writeln!(report, "threshold_candidates=0.00..1.00_step_0.01")
        .expect("writing String cannot fail");
    writeln!(report, "threshold_source=validation").expect("writing String cannot fail");
    writeln!(report, "imputation=train_mean").expect("writing String cannot fail");
    writeln!(report, "standardization=train_mean_population_std")
        .expect("writing String cannot fail");
    writeln!(report, "regularization=l2 lambda={L2_LAMBDA:.6}")
        .expect("writing String cannot fail");
    writeln!(
        report,
        "optimizer=batch_gradient_descent learning_rate={LEARNING_RATE:.6} max_epochs={MAX_EPOCHS}"
    )
    .expect("writing String cannot fail");
    writeln!(report, "test_policy=one_pass_after_threshold_freeze")
        .expect("writing String cannot fail");

    let mut result_rows = 0;
    for (variant_name, feature_names) in VARIANTS {
        let indices = feature_indices(feature_names);
        let scaler = fit_scaler(&train, &indices);
        let model = LogisticModel::fit(&train, &indices, &scaler)?;
        let train_scores = train
            .iter()
            .map(|row| model.score(row, &indices, &scaler))
            .collect::<Vec<_>>();
        let validation_scores = validation
            .iter()
            .map(|row| model.score(row, &indices, &scaler))
            .collect::<Vec<_>>();
        let test_scores = test
            .iter()
            .map(|row| model.score(row, &indices, &scaler))
            .collect::<Vec<_>>();
        let threshold = select_threshold(&validation, &validation_scores);

        writeln!(
            report,
            "variant={variant_name} threshold={threshold:.2} feature_count={} features={}",
            feature_names.len(),
            feature_names.join("|")
        )
        .expect("writing String cannot fail");
        writeln!(
            report,
            "parameters variant={variant_name} bias={:.12} weights={}",
            model.bias,
            model
                .weights
                .iter()
                .map(|weight| format!("{weight:.12}"))
                .collect::<Vec<_>>()
                .join("|")
        )
        .expect("writing String cannot fail");
        writeln!(
            report,
            "scaler variant={variant_name} means={} scales={}",
            scaler
                .means
                .iter()
                .map(|value| format!("{value:.12}"))
                .collect::<Vec<_>>()
                .join("|"),
            scaler
                .scales
                .iter()
                .map(|value| format!("{value:.12}"))
                .collect::<Vec<_>>()
                .join("|")
        )
        .expect("writing String cannot fail");
        writeln!(
            report,
            "{}",
            metric_line(
                variant_name,
                "train",
                metrics(&train, &train_scores, threshold),
                threshold,
                train.len()
            )
        )
        .expect("writing String cannot fail");
        writeln!(
            report,
            "{}",
            metric_line(
                variant_name,
                "validation",
                metrics(&validation, &validation_scores, threshold),
                threshold,
                validation.len()
            )
        )
        .expect("writing String cannot fail");
        writeln!(
            report,
            "{}",
            metric_line(
                variant_name,
                "test",
                metrics(&test, &test_scores, threshold),
                threshold,
                test.len()
            )
        )
        .expect("writing String cannot fail");

        for (split_name, split_rows, scores) in [
            ("train", &train, &train_scores),
            ("validation", &validation, &validation_scores),
            ("test", &test, &test_scores),
        ] {
            for (row, score) in split_rows.iter().zip(scores) {
                let predicted = i32::from(*score >= threshold);
                writeln!(
                    results,
                    "{variant_name},{},{split_name},{},{},{score:.12},{predicted},{threshold:.2},{}",
                    csv_escape(&row.event_id),
                    csv_escape(&row.ts),
                    row.actual,
                    feature_names.len(),
                )
                .expect("writing String cannot fail");
                result_rows += 1;
            }
        }
    }

    if let Some(parent) = results_path
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
    std::fs::write(results_path, results)?;
    std::fs::write(report_path, report)?;
    Ok(ModelSummary {
        variants: VARIANTS.len(),
        result_rows,
    })
}

/// Fit the three frozen ablations and write predictions plus a reproducible report.
pub fn run_models(
    labels_path: &Path,
    split_path: &Path,
    results_path: &Path,
    report_path: &Path,
) -> Result<ModelSummary, Box<dyn std::error::Error>> {
    let rows = parse_label_rows(labels_path)?;
    let splits = parse_splits(split_path)?;
    for row in &rows {
        if !splits.contains_key(&row.event_id) {
            return Err(format!("missing split for {}", row.event_id).into());
        }
    }
    write_model_outputs(&rows, &splits, results_path, report_path)
}

#[cfg(test)]
mod tests {
    use super::{Confusion, metrics, sigmoid};

    #[test]
    fn sigmoid_stays_finite_at_large_logits() {
        assert!(sigmoid(1_000.0) <= 1.0);
        assert!(sigmoid(-1_000.0) >= 0.0);
    }

    #[test]
    fn metrics_counts_all_four_confusion_cells() {
        let confusion = Confusion {
            true_positive: 2,
            false_positive: 1,
            true_negative: 3,
            false_negative: 1,
        };
        assert_eq!(
            confusion.true_positive
                + confusion.false_positive
                + confusion.true_negative
                + confusion.false_negative,
            7
        );
        let _ = metrics;
    }
}

use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use slopeguard::analysis::{
    BaselineConfig, build_features, generate_labels, write_baseline_results, write_features_csv,
    write_split_manifest,
};
use slopeguard::ingest::{Report, Store, ingest_lines, latest_health, load_jsonl};
use slopeguard::metrics::run_metrics;
use slopeguard::model::run_models;
use slopeguard::receiver::{Server, Tokens};
use slopeguard::replay::{build_stable_dry, write_jsonl};

#[derive(Parser)]
#[command(
    name = "slopeguard",
    about = "SlopeGuard telemetry ingest and deterministic replay"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate and store telemetry JSONL.
    Ingest {
        #[arg(required = true)]
        files: Vec<String>,
        #[arg(long, default_value = "data/raw/slopeguard.db")]
        db: String,
        #[arg(long, help = "delete the db first, for a clean demo run")]
        reset: bool,
    },
    /// Write deterministic replay fixtures.
    Replay {
        #[arg(long, default_value = "data/replays/stable_dry.jsonl")]
        out: String,
        /// packets per node
        #[arg(long, default_value_t = 20)]
        n: usize,
    },
    /// Serve telemetry over HTTP for device posts.
    Serve {
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,
        #[arg(long, default_value = "data/raw/slopeguard.db")]
        db: String,
        #[arg(long, default_value = "config/nodes.tokens")]
        tokens: String,
    },
    /// Build leakage-safe feature windows from accepted raw telemetry.
    Features {
        #[arg(long, default_value = "data/raw/slopeguard.db")]
        db: String,
        #[arg(long, default_value = "data/processed/features.csv")]
        out: String,
    },
    /// Generate labels from independent displacement evidence.
    Labels {
        #[arg(long, default_value = "data/processed/features.csv")]
        features: String,
        #[arg(long, default_value = "data/experiments/events.csv")]
        events: String,
        #[arg(long, default_value = "data/raw/ground_truth.csv")]
        truth: String,
        #[arg(long, default_value_t = 1.0)]
        threshold_mm: f64,
        #[arg(long, default_value = "data/labels/labels.csv")]
        out: String,
    },
    /// Assign complete events to chronological train/validation/test splits.
    Split {
        #[arg(long, default_value = "data/labels/labels.csv")]
        labels: String,
        #[arg(long, default_value = "data/validation/split_manifest.csv")]
        out: String,
    },
    /// Emit B0/B1/B2 transparent baseline predictions on the frozen split.
    Baselines {
        #[arg(long, default_value = "data/labels/labels.csv")]
        labels: String,
        #[arg(long, default_value = "data/validation/split_manifest.csv")]
        split: String,
        #[arg(long, default_value = "data/validation/baseline_results.csv")]
        out: String,
    },
    /// Fit train-only logistic models, select validation thresholds, and write test predictions.
    Models {
        #[arg(long, default_value = "data/labels/labels.csv")]
        labels: String,
        #[arg(long, default_value = "data/validation/split_manifest.csv")]
        split: String,
        #[arg(long, default_value = "data/validation/model_results.csv")]
        out: String,
        #[arg(long, default_value = "data/validation/model_report.txt")]
        report: String,
    },
    /// Evaluate baselines and model ablations with event-level diagnostics.
    Metrics {
        #[arg(long, default_value = "data/labels/labels.csv")]
        labels: String,
        #[arg(long, default_value = "data/validation/split_manifest.csv")]
        split: String,
        #[arg(long, default_value = "data/validation/baseline_results.csv")]
        baselines: String,
        #[arg(long, default_value = "data/validation/model_results.csv")]
        models: String,
        #[arg(long, default_value = "data/validation/metrics.csv")]
        out: String,
        #[arg(long, default_value = "data/validation/metrics_report.txt")]
        report: String,
        #[arg(long, default_value = "synthetic")]
        source_kind: String,
    },
}

fn errors_json(report: &Report) -> String {
    let pairs: Vec<String> = report
        .errors
        .iter()
        .map(|(code, count)| format!("\"{code}\": {count}"))
        .collect();
    format!("{{{}}}", pairs.join(", "))
}

fn ingest(files: &[String], db: &str, reset: bool) -> ExitCode {
    if reset {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{db}{suffix}"));
        }
    }
    let mut store = match Store::open(db) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    let (mut accepted, mut rejected) = (0, 0);
    for path in files {
        let lines = match load_jsonl(path) {
            Ok(lines) => lines,
            Err(e) => {
                eprintln!("error: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let report = match ingest_lines(&mut store, &lines, path) {
            Ok(report) => report,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(1);
            }
        };
        println!(
            "{path}: accepted={} rejected={} duplicates={} out_of_order={}",
            report.accepted, report.rejected, report.duplicates, report.out_of_order
        );
        if !report.errors.is_empty() {
            println!("  errors: {}", errors_json(&report));
        }
        accepted += report.accepted;
        rejected += report.rejected;
    }
    println!("db={db} accepted={accepted} rejected={rejected}");
    match latest_health(&store) {
        Ok(health) => {
            for h in health {
                let batt = h
                    .battery_v
                    .map(|v| format!("{v:?}"))
                    .unwrap_or_else(|| "None".to_string());
                println!(
                    "  {:<9} {:<12} seq={:<4} ts={} status={} batt={batt}V",
                    h.node_role, h.node_id, h.seq, h.ts_device, h.sensor_status
                );
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    }
    if accepted > 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn replay(out: &str, n: usize) -> ExitCode {
    match write_jsonl(&build_stable_dry(n), out) {
        Ok(()) => {
            println!("wrote {} packets to {out}", n * 2);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn serve(addr: &str, db: &str, tokens_path: &str) -> ExitCode {
    let tokens = match Tokens::load(tokens_path) {
        Ok(tokens) => tokens,
        Err(e) => {
            eprintln!("error: tokens {tokens_path}: {e}");
            return ExitCode::from(2);
        }
    };
    let count = tokens.len();
    let store = match Store::open(db) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    let server = match Server::bind(addr, store, tokens) {
        Ok(server) => server,
        Err(e) => {
            eprintln!("error: bind {addr}: {e}");
            return ExitCode::from(1);
        }
    };
    match server.local_addr() {
        Ok(a) => println!("listening on http://{a} db={db} nodes={count}"),
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    }
    server.serve();
    ExitCode::SUCCESS
}

fn features(db: &str, out: &str) -> ExitCode {
    let store = match Store::open(db) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    match build_features(&store).and_then(|rows| {
        write_features_csv(&rows, Path::new(out))
            .map_err(Into::into)
            .map(|()| rows.len())
    }) {
        Ok(count) => {
            println!("features={count} out={out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn labels(features: &str, events: &str, truth: &str, threshold_mm: f64, out: &str) -> ExitCode {
    match generate_labels(
        Path::new(features),
        Path::new(events),
        Path::new(truth),
        threshold_mm,
        Path::new(out),
    ) {
        Ok(count) => {
            println!("labels={count} out={out} threshold_mm={threshold_mm}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn split(labels: &str, out: &str) -> ExitCode {
    match write_split_manifest(Path::new(labels), Path::new(out)) {
        Ok(count) => {
            println!("events={count} split={out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn baselines(labels: &str, split: &str, out: &str) -> ExitCode {
    match write_baseline_results(
        Path::new(labels),
        Path::new(split),
        Path::new(out),
        BaselineConfig::default(),
    ) {
        Ok(count) => {
            println!("baseline_rows={count} out={out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn models(labels: &str, split: &str, out: &str, report: &str) -> ExitCode {
    match run_models(
        Path::new(labels),
        Path::new(split),
        Path::new(out),
        Path::new(report),
    ) {
        Ok(summary) => {
            println!(
                "model_variants={} result_rows={} out={} report={}",
                summary.variants, summary.result_rows, out, report
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn metrics(
    labels: &str,
    split: &str,
    baselines: &str,
    models: &str,
    out: &str,
    report: &str,
    source_kind: &str,
) -> ExitCode {
    match run_metrics(
        Path::new(labels),
        Path::new(split),
        Path::new(baselines),
        Path::new(models),
        Path::new(out),
        Path::new(report),
        source_kind,
    ) {
        Ok(summary) => {
            println!(
                "metric_methods={} metric_rows={} out={} report={} source_kind={}",
                summary.methods, summary.rows, out, report, source_kind
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Ingest { files, db, reset } => ingest(&files, &db, reset),
        Command::Replay { out, n } => replay(&out, n),
        Command::Serve { addr, db, tokens } => serve(&addr, &db, &tokens),
        Command::Features { db, out } => features(&db, &out),
        Command::Labels {
            features,
            events,
            truth,
            threshold_mm,
            out,
        } => labels(&features, &events, &truth, threshold_mm, &out),
        Command::Split { labels, out } => split(&labels, &out),
        Command::Baselines { labels, split, out } => baselines(&labels, &split, &out),
        Command::Models {
            labels,
            split,
            out,
            report,
        } => models(&labels, &split, &out, &report),
        Command::Metrics {
            labels,
            split,
            baselines,
            models,
            out,
            report,
            source_kind,
        } => metrics(
            &labels,
            &split,
            &baselines,
            &models,
            &out,
            &report,
            &source_kind,
        ),
    }
}

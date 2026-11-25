//! Deterministic replay fixtures, so demos and judging never depend on hardware.
//!
//! ```text
//! slopeguard replay --out data/replays/stable_dry.jsonl
//! ```
//!
//! Every value is a pure function of the packet index: same bytes every run.

use std::fmt::Write as _;
use std::path::Path;

use chrono::{FixedOffset, SecondsFormat, TimeZone};
use serde_json::{Map, Value, json};

pub const SITE: &str = "SLOPE-001";
pub const INTERVAL_S: i64 = 10;
pub const NODE_MOVE: &str = "IMU-MOVE-01";
pub const NODE_REF: &str = "IMU-REF-01";
pub const CALIBRATION: &str = "cal-2026-11-25";
pub const FIRMWARE: &str = "0.1.0";

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Unit-gravity accelerometer reading at a static pitch/roll, in g.
fn accel(pitch: f64, roll: f64) -> (f64, f64, f64) {
    let (p, r) = (pitch.to_radians(), roll.to_radians());
    (
        round3(r.sin()),
        round3(r.cos() * p.sin()),
        round3(r.cos() * p.cos()),
    )
}

fn ts(index: usize) -> String {
    let base = FixedOffset::east_opt(7 * 3600)
        .expect("INFALLIBLE: +07:00 is a valid offset")
        .with_ymd_and_hms(2026, 11, 25, 8, 0, 0)
        .single()
        .expect("INFALLIBLE: 2026-11-25T08:00:00+07:00 is a valid local time");
    (base + chrono::TimeDelta::seconds(INTERVAL_S * index as i64))
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}

#[allow(clippy::too_many_arguments)]
fn packet(
    node_id: &str,
    role: &str,
    i: usize,
    pitch: f64,
    roll: f64,
    soil_pct: Option<f64>,
    soil_raw: Option<i64>,
    rain: Option<f64>,
    temp: f64,
) -> Value {
    let (ax, ay, az) = accel(pitch, roll);
    let mut p = Map::new();
    let fields: [(&str, Value); 24] = [
        ("schema_version", json!(1)),
        ("site_id", json!(SITE)),
        ("node_id", json!(node_id)),
        ("node_role", json!(role)),
        ("seq", json!(i + 1)),
        ("ts_device", json!(ts(i))),
        ("accel_x_g", json!(ax)),
        ("accel_y_g", json!(ay)),
        ("accel_z_g", json!(az)),
        ("tilt_pitch_deg", json!(round3(pitch))),
        ("tilt_roll_deg", json!(round3(roll))),
        ("gyro_x_dps", json!(0.0)),
        ("gyro_y_dps", json!(0.0)),
        ("gyro_z_dps", json!(0.0)),
        ("rain_mm_interval", json!(rain)),
        ("soil_moisture_pct", json!(soil_pct)),
        ("soil_moisture_raw", json!(soil_raw)),
        ("probe_depth_cm", json!(20)),
        ("temp_c", json!(temp)),
        ("battery_v", json!(round3(4.01 - 0.001 * i as f64))),
        ("signal_dbm", json!(-76)),
        ("sensor_status", json!("OK")),
        ("firmware", json!(FIRMWARE)),
        ("calibration_version", json!(CALIBRATION)),
    ];
    for (k, v) in fields {
        p.insert(k.to_string(), v);
    }
    Value::Object(p)
}

/// Stable dry control: no rain, soil low, reference steadier than movement.
///
/// ponytail: synthetic packets prove the pipeline, not the slope. Replace with
/// recorded rig data before any metric is reported.
pub fn build_stable_dry(n: usize) -> Vec<Value> {
    let mut packets = Vec::with_capacity(n * 2);
    for i in 0..n {
        let i_f = i as f64;
        packets.push(packet(
            NODE_MOVE,
            "movement",
            i,
            1.80 + 0.06 * (i_f / 2.0).sin(),
            -0.41 + 0.05 * (i_f / 3.0).sin(),
            Some(round2(34.0 + 0.05 * i_f)),
            Some(1400 + 4 * i as i64),
            Some(0.0),
            round2(26.5 + 0.02 * i_f),
        ));
        packets.push(packet(
            NODE_REF,
            "reference",
            i,
            0.05 + 0.005 * (i_f / 3.0).sin(),
            0.10 + 0.004 * (i_f / 4.0).cos(),
            None,
            None,
            None,
            round2(26.4 + 0.02 * i_f),
        ));
    }
    packets
}

fn write_value(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => {
            let quoted = serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
            out.push_str(&quoted);
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let quoted = serde_json::to_string(k).unwrap_or_else(|_| "\"\"".to_string());
                out.push_str(&quoted);
                out.push_str(": ");
                write_value(out, &map[k]);
            }
            out.push('}');
        }
    }
}

/// `json.dumps(p, sort_keys=True)` formatting: `, ` / `: ` separators, sorted keys.
pub fn python_json(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v);
    out
}

pub fn write_jsonl(packets: &[Value], path: &str) -> std::io::Result<()> {
    if let Some(parent) = Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = String::new();
    for p in packets {
        let _ = writeln!(out, "{}", python_json(p));
    }
    std::fs::write(path, out)
}

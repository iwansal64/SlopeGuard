//! Frozen telemetry contract v1. Authoritative doc: docs/system_contract.md.
//!
//! Reject-at-the-boundary rules live here so device, replay, receiver, tests,
//! and later feature code agree on one definition of a valid packet. This is
//! the only implementation: there is no second language to keep in sync.

use chrono::DateTime;
use serde_json::Value;

pub const SCHEMA_VERSION: i64 = 1;
pub const HORIZON_MINUTES: i64 = 30;

pub const NODE_ROLES: [&str; 2] = ["movement", "reference"];
pub const SENSOR_STATUS: [&str; 3] = ["OK", "FAULT", "STALE"];
pub const STATES: [&str; 5] = ["STABLE", "WATCH", "MOVEMENT_RISK", "CRITICAL", "UNKNOWN"];

// Static tilt is gravity-referenced; anything far from 1 g is a fault, not motion.
pub const GRAVITY_MIN: f64 = 0.3;
pub const GRAVITY_MAX: f64 = 1.7;

// field -> (min, max); null = sensor absent, never zero
pub const FLOAT_RANGES: [(&str, f64, f64); 15] = [
    ("accel_x_g", -2.0, 2.0),
    ("accel_y_g", -2.0, 2.0),
    ("accel_z_g", -2.0, 2.0),
    ("tilt_pitch_deg", -90.0, 90.0),
    ("tilt_roll_deg", -90.0, 90.0),
    ("gyro_x_dps", -2000.0, 2000.0),
    ("gyro_y_dps", -2000.0, 2000.0),
    ("gyro_z_dps", -2000.0, 2000.0),
    ("rain_mm_interval", 0.0, 50.0),
    ("soil_moisture_raw", 0.0, 4095.0),
    ("soil_moisture_pct", 0.0, 100.0),
    ("probe_depth_cm", 0.0, 200.0),
    ("temp_c", -20.0, 60.0),
    ("battery_v", 3.0, 4.5),
    ("signal_dbm", -120.0, 0.0),
];
pub const ACCEL: [&str; 3] = ["accel_x_g", "accel_y_g", "accel_z_g"];
pub const REQUIRED_FLOATS: [&str; 7] = [
    "accel_x_g",
    "accel_y_g",
    "accel_z_g",
    "tilt_pitch_deg",
    "tilt_roll_deg",
    "battery_v",
    "signal_dbm",
];
pub const OPTIONAL_FLOATS: [&str; 8] = [
    "gyro_x_dps",
    "gyro_y_dps",
    "gyro_z_dps",
    "rain_mm_interval",
    "soil_moisture_raw",
    "soil_moisture_pct",
    "probe_depth_cm",
    "temp_c",
];

// minimum packet: identity, ordering, calibrated tilt, and health. Environmental
// fields ride along when that node has those sensors.
pub const REQUIRED: [&str; 16] = [
    "schema_version",
    "site_id",
    "node_id",
    "node_role",
    "seq",
    "ts_device",
    "accel_x_g",
    "accel_y_g",
    "accel_z_g",
    "tilt_pitch_deg",
    "tilt_roll_deg",
    "battery_v",
    "signal_dbm",
    "sensor_status",
    "firmware",
    "calibration_version",
];
pub const KNOWN: [&str; 24] = [
    "schema_version",
    "site_id",
    "node_id",
    "node_role",
    "seq",
    "ts_device",
    "accel_x_g",
    "accel_y_g",
    "accel_z_g",
    "tilt_pitch_deg",
    "tilt_roll_deg",
    "gyro_x_dps",
    "gyro_y_dps",
    "gyro_z_dps",
    "rain_mm_interval",
    "soil_moisture_raw",
    "soil_moisture_pct",
    "probe_depth_cm",
    "temp_c",
    "battery_v",
    "signal_dbm",
    "sensor_status",
    "firmware",
    "calibration_version",
];

fn valid_id(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=32).contains(&b.len())
        && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit())
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

fn valid_version(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=32).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(*c, b'.' | b'_' | b'+' | b'-'))
}

/// RFC 3339 with an explicit offset or `Z`: the only timestamp form the wire
/// accepts. Devices are ours, so one unambiguous rule beats permissive parsing;
/// anything else is rejected and logged with the raw payload.
fn valid_ts(s: &str) -> bool {
    // chrono is lenient about the date/time separator; the contract is not.
    if !matches!(s.as_bytes().get(10), Some(b'T' | b't')) {
        return false;
    }
    DateTime::parse_from_rfc3339(s).is_ok()
        // RFC 3339 makes the trailing marker case-insensitive; chrono does not.
        || s.strip_suffix('z')
            .is_some_and(|rest| DateTime::parse_from_rfc3339(&format!("{rest}Z")).is_ok())
}

/// A plain JSON number. Non-finite values cannot appear in JSON text: `NaN` and
/// `Infinity` are malformed JSON and are rejected before this runs.
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Return a sorted list of error codes; empty = packet is acceptable.
///
/// A required key that is missing OR null is `missing:<key>`: null is not a
/// value the contract allows, and rejecting it here keeps it out of the NOT
/// NULL insert where it would surface as a misleading duplicate.
pub fn validate(p: &Value) -> Vec<String> {
    let Value::Object(obj) = p else {
        return vec!["not_object".to_string()];
    };
    let mut errors = Vec::new();

    for key in REQUIRED {
        if obj.get(key).is_none_or(Value::is_null) {
            errors.push(format!("missing:{key}"));
        }
    }
    for key in obj.keys() {
        if !KNOWN.contains(&key.as_str()) {
            errors.push(format!("unknown:{key}"));
        }
    }

    for key in ["site_id", "node_id"] {
        if let Some(v) = obj.get(key).filter(|v| !v.is_null())
            && !v.as_str().is_some_and(valid_id)
        {
            errors.push(format!("bad:{key}"));
        }
    }
    for key in ["firmware", "calibration_version"] {
        if let Some(v) = obj.get(key).filter(|v| !v.is_null())
            && !v.as_str().is_some_and(valid_version)
        {
            errors.push(format!("bad:{key}"));
        }
    }
    if let Some(v) = obj.get("schema_version").filter(|v| !v.is_null())
        && (v.as_i64() != Some(SCHEMA_VERSION) || v.is_f64())
    {
        errors.push("bad:schema_version".to_string());
    }
    if let Some(v) = obj.get("node_role").filter(|v| !v.is_null())
        && !v.as_str().is_some_and(|s| NODE_ROLES.contains(&s))
    {
        errors.push("bad:node_role".to_string());
    }
    if let Some(v) = obj.get("sensor_status").filter(|v| !v.is_null())
        && !v.as_str().is_some_and(|s| SENSOR_STATUS.contains(&s))
    {
        errors.push("bad:sensor_status".to_string());
    }
    if let Some(v) = obj.get("ts_device").filter(|v| !v.is_null())
        && !v.as_str().is_some_and(valid_ts)
    {
        errors.push("bad:ts_device".to_string());
    }
    if let Some(v) = obj.get("seq").filter(|v| !v.is_null()) {
        match v.as_i64() {
            Some(n) if !v.is_f64() => {
                if n < 0 {
                    errors.push("range:seq".to_string());
                }
            }
            _ => errors.push("bad:seq".to_string()),
        }
    }

    for (key, lo, hi) in FLOAT_RANGES {
        let Some(v) = obj.get(key).filter(|v| !v.is_null()) else {
            continue;
        };
        match number(v) {
            Some(x) if x.is_finite() && lo <= x && x <= hi => {}
            _ => errors.push(format!("range:{key}")),
        }
    }

    let accel: Option<Vec<f64>> = ACCEL.iter().map(|k| obj.get(*k).and_then(number)).collect();
    if let Some(values) = accel {
        let mag = values.iter().map(|x| x * x).sum::<f64>().sqrt();
        if !mag.is_finite() || !(GRAVITY_MIN..=GRAVITY_MAX).contains(&mag) {
            errors.push("gravity".to_string());
        }
    }

    errors.sort();
    errors
}

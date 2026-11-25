use serde_json::{Value, json};
use slopeguard::contract::validate;

fn packet() -> Value {
    json!({
        "schema_version": 1,
        "site_id": "SLOPE-001",
        "node_id": "IMU-MOVE-01",
        "node_role": "movement",
        "seq": 1,
        "ts_device": "2026-11-25T08:30:00+07:00",
        "accel_x_g": 0.012,
        "accel_y_g": 0.684,
        "accel_z_g": 0.729,
        "tilt_pitch_deg": 1.84,
        "tilt_roll_deg": -0.41,
        "battery_v": 4.01,
        "signal_dbm": -76,
        "sensor_status": "OK",
        "firmware": "0.1.0",
        "calibration_version": "cal-2026-11-25"
    })
}

fn errors(p: &Value) -> Vec<String> {
    validate(p)
}

#[test]
fn valid_packet() {
    assert_eq!(errors(&packet()), Vec::<String>::new());
}

#[test]
fn optional_fields() {
    let mut p = packet();
    let obj = p.as_object_mut().unwrap();
    obj.insert("rain_mm_interval".into(), json!(3.2));
    obj.insert("soil_moisture_pct".into(), json!(68.4));
    obj.insert("soil_moisture_raw".into(), json!(2100));
    obj.insert("gyro_x_dps".into(), json!(0.08));
    obj.insert("temp_c".into(), json!(26.5));
    obj.insert("probe_depth_cm".into(), json!(20));
    assert_eq!(errors(&p), Vec::<String>::new());
}

#[test]
fn missing_required() {
    let mut p = packet();
    p.as_object_mut().unwrap().remove("accel_y_g");
    assert!(errors(&p).contains(&"missing:accel_y_g".to_string()));
}

#[test]
fn null_required_rejected() {
    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("node_role".into(), Value::Null);
    assert!(errors(&p).contains(&"missing:node_role".to_string()));
}

#[test]
fn unknown_field_rejected() {
    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("x_extra".into(), json!(1));
    assert!(errors(&p).contains(&"unknown:x_extra".to_string()));
}

#[test]
fn schema_version() {
    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("schema_version".into(), json!(2));
    assert!(errors(&p).contains(&"bad:schema_version".to_string()));

    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("schema_version".into(), json!(1.0));
    assert!(errors(&p).contains(&"bad:schema_version".to_string()));
}

#[test]
fn timestamp_forms() {
    // The wire accepts RFC 3339 (offset or Z) only.
    for ts in [
        "2026-11-25T08:30:00Z",
        "2026-11-25T08:30:00z",
        "2026-11-25T08:30:00+07:00",
        "2026-11-25T08:30:00.123+07:00",
        "2026-11-25T08:30:00.5Z",
    ] {
        let mut p = packet();
        p.as_object_mut()
            .unwrap()
            .insert("ts_device".into(), json!(ts));
        assert_eq!(errors(&p), Vec::<String>::new(), "{ts}");
    }
}

#[test]
fn bad_id() {
    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("site_id".into(), json!("slope 001"));
    assert!(errors(&p).contains(&"bad:site_id".to_string()));

    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("node_id".into(), json!("SLOPE-001\n"));
    assert!(errors(&p).contains(&"bad:node_id".to_string()));
}

#[test]
fn bad_role() {
    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("node_role".into(), json!("moving"));
    assert!(errors(&p).contains(&"bad:node_role".to_string()));
}

#[test]
fn range() {
    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("tilt_pitch_deg".into(), json!(91));
    assert!(errors(&p).contains(&"range:tilt_pitch_deg".to_string()));

    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("battery_v".into(), json!(0.0));
    assert!(errors(&p).contains(&"range:battery_v".to_string()));
}

#[test]
fn impossible_gravity() {
    // 2g magnitude cannot be a static accelerometer reading.
    let mut p = packet();
    let obj = p.as_object_mut().unwrap();
    obj.insert("accel_x_g".into(), json!(2.0));
    obj.insert("accel_y_g".into(), json!(0.0));
    obj.insert("accel_z_g".into(), json!(0.0));
    assert!(errors(&p).contains(&"gravity".to_string()));
}

#[test]
fn nonfinite() {
    // JSON text cannot carry NaN/inf; serde rejects the bare literals outright.
    assert!(serde_json::from_str::<Value>(r#"{"accel_z_g": NaN}"#).is_err());
    assert!(serde_json::from_str::<Value>(r#"{"accel_z_g": Infinity}"#).is_err());

    // An object is not a number, so a marker-shaped value is a range error.
    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("accel_z_g".into(), json!({"$num": "NaN"}));
    assert_eq!(errors(&p), vec!["range:accel_z_g".to_string()]);
}

#[test]
fn non_rfc3339_timestamps_rejected() {
    // Every offset-free, partial, or non-RFC 3339 form is rejected at the
    // boundary and logged with the raw payload.
    for ts in [
        "2026-11-25T08:30:00",          // no offset
        "2026-11-25T08:30+07:00",       // no seconds
        "2026-11-25 08:30:00+07:00",    // space separator
        "20261125T083000+0700",         // basic format
        "2026-11-25T08:30:00,5+07:00",  // comma fraction
        "2026-11-25T08:30:00+07",       // hour-only offset
        "2026-11-25T08:30:00+07:00:30", // offset seconds
        "2026-W48-3T08:30:00+07:00",    // ISO week date
        "2026-11-25T24:00:00+07:00",    // 24:00
        "25-11-2026 08:30",             // not ISO 8601
    ] {
        let mut p = packet();
        p.as_object_mut()
            .unwrap()
            .insert("ts_device".into(), json!(ts));
        assert!(
            errors(&p).contains(&"bad:ts_device".to_string()),
            "{ts} was accepted"
        );
    }
}

#[test]
fn bool_is_not_int() {
    let mut p = packet();
    p.as_object_mut().unwrap().insert("seq".into(), json!(true));
    assert!(errors(&p).contains(&"bad:seq".to_string()));

    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("battery_v".into(), json!(false));
    assert!(errors(&p).contains(&"range:battery_v".to_string()));
}

#[test]
fn negative_seq() {
    let mut p = packet();
    p.as_object_mut().unwrap().insert("seq".into(), json!(-1));
    assert!(errors(&p).contains(&"range:seq".to_string()));
}

#[test]
fn status_enum() {
    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("sensor_status".into(), json!("ok"));
    assert!(errors(&p).contains(&"bad:sensor_status".to_string()));
}

#[test]
fn null_optional_ok() {
    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("rain_mm_interval".into(), Value::Null);
    assert_eq!(errors(&p), Vec::<String>::new());
}

#[test]
fn json_roundtrip_values() {
    let mut p = packet();
    p.as_object_mut()
        .unwrap()
        .insert("rain_mm_interval".into(), json!(0.0));
    let text = serde_json::to_string(&p).unwrap();
    let back: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(errors(&back), Vec::<String>::new());
}

#[test]
fn not_object() {
    assert_eq!(validate(&json!("x")), vec!["not_object".to_string()]);
    assert_eq!(validate(&json!([1, 2])), vec!["not_object".to_string()]);
}

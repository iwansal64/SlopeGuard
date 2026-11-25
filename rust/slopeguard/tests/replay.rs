use slopeguard::contract::validate;
use slopeguard::replay::{build_stable_dry, python_json};

#[test]
fn deterministic() {
    let a: Vec<String> = build_stable_dry(20).iter().map(python_json).collect();
    let b: Vec<String> = build_stable_dry(20).iter().map(python_json).collect();
    assert_eq!(a, b);
}

#[test]
fn all_packets_valid() {
    for p in build_stable_dry(20) {
        assert_eq!(
            validate(&p),
            Vec::<String>::new(),
            "{}:{}",
            p["node_id"],
            p["seq"]
        );
    }
}

#[test]
fn shape() {
    let packets = build_stable_dry(20);
    assert_eq!(packets.len(), 40);
    let mut nodes: Vec<&str> = packets
        .iter()
        .map(|p| p["node_id"].as_str().unwrap())
        .collect();
    nodes.sort();
    nodes.dedup();
    assert_eq!(nodes, ["IMU-MOVE-01", "IMU-REF-01"]);

    for node in ["IMU-MOVE-01", "IMU-REF-01"] {
        let seqs: Vec<i64> = packets
            .iter()
            .filter(|p| p["node_id"] == node)
            .map(|p| p["seq"].as_i64().unwrap())
            .collect();
        assert_eq!(seqs, (1..=20).collect::<Vec<_>>());
    }
}

#[test]
fn reference_more_stable_than_movement() {
    let packets = build_stable_dry(20);
    let spread = |role: &str| {
        let v: Vec<f64> = packets
            .iter()
            .filter(|p| p["node_role"] == role)
            .map(|p| p["tilt_pitch_deg"].as_f64().unwrap())
            .collect();
        v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
    };
    assert!(spread("reference") < spread("movement"));
}

#[test]
fn gravity_consistent() {
    // per-axis rounding to 3 dp allows ~0.003 g magnitude error
    for p in build_stable_dry(20) {
        let mag = (p["accel_x_g"].as_f64().unwrap().powi(2)
            + p["accel_y_g"].as_f64().unwrap().powi(2)
            + p["accel_z_g"].as_f64().unwrap().powi(2))
        .sqrt();
        assert!((mag - 1.0).abs() < 0.003, "magnitude {mag}");
    }
}

#[test]
fn timestamps_ordered_within_node() {
    let packets = build_stable_dry(20);
    for node in ["IMU-MOVE-01", "IMU-REF-01"] {
        let ts: Vec<&str> = packets
            .iter()
            .filter(|p| p["node_id"] == node)
            .map(|p| p["ts_device"].as_str().unwrap())
            .collect();
        let mut sorted = ts.clone();
        sorted.sort();
        assert_eq!(ts, sorted);
        assert!(ts.iter().all(|t| t.ends_with("+07:00")));
    }
}

#[test]
fn json_serializable() {
    for p in build_stable_dry(20) {
        serde_json::to_string(&p).unwrap();
    }
}

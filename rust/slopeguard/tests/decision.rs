use slopeguard::decision::{DecisionConfig, DecisionEngine, DecisionState, Observation};

fn config() -> DecisionConfig {
    DecisionConfig {
        warning_threshold: 0.5,
        critical_threshold: 0.8,
        exit_threshold: 0.3,
        confirmation_windows: 2,
        clear_windows: 2,
    }
}

fn observation(score: Option<f64>) -> Observation {
    Observation {
        risk_score: score,
        data_ok: true,
        environment_signal: false,
        motion_signal: false,
        independent_movement: false,
    }
}

#[test]
fn risk_requires_confirmation_then_critical_requires_its_own_confirmation() {
    let mut engine = DecisionEngine::new(config()).unwrap();

    let first = engine.observe(observation(Some(0.6)));
    assert_eq!(first.state, DecisionState::Watch);
    assert!(first.reason.contains("confirm"));

    let second = engine.observe(observation(Some(0.6)));
    assert_eq!(second.state, DecisionState::MovementRisk);

    let third = engine.observe(observation(Some(0.95)));
    assert_eq!(third.state, DecisionState::MovementRisk);
    assert!(third.reason.contains("confirm"));

    let fourth = engine.observe(observation(Some(0.95)));
    assert_eq!(fourth.state, DecisionState::Critical);
}

#[test]
fn risk_does_not_clear_until_low_score_persists_through_hysteresis() {
    let mut engine = DecisionEngine::new(config()).unwrap();
    engine.observe(observation(Some(0.6)));
    engine.observe(observation(Some(0.6)));

    let first_low = engine.observe(observation(Some(0.2)));
    assert_eq!(first_low.state, DecisionState::MovementRisk);
    assert!(first_low.reason.contains("hysteresis"));

    let middle = engine.observe(observation(Some(0.4)));
    assert_eq!(middle.state, DecisionState::MovementRisk);

    let second_low = engine.observe(observation(Some(0.2)));
    assert_eq!(second_low.state, DecisionState::MovementRisk);

    let third_low = engine.observe(observation(Some(0.2)));
    assert_eq!(third_low.state, DecisionState::Stable);
}

#[test]
fn critical_state_does_not_downgrade_on_one_intermediate_score() {
    let mut engine = DecisionEngine::new(config()).unwrap();
    engine.observe(observation(Some(0.95)));
    engine.observe(observation(Some(0.95)));
    assert_eq!(engine.state(), DecisionState::Critical);

    let result = engine.observe(observation(Some(0.6)));
    assert_eq!(result.state, DecisionState::Critical);
    assert!(result.reason.contains("hysteresis"));
}

#[test]
fn stale_or_invalid_data_is_unknown_and_independent_movement_is_critical() {
    let mut engine = DecisionEngine::new(config()).unwrap();

    let stale = engine.observe(Observation {
        data_ok: false,
        ..observation(Some(0.9))
    });
    assert_eq!(stale.state, DecisionState::Unknown);

    let invalid = engine.observe(observation(Some(f64::NAN)));
    assert_eq!(invalid.state, DecisionState::Unknown);

    let independent = engine.observe(Observation {
        risk_score: Some(0.1),
        independent_movement: true,
        ..observation(Some(0.1))
    });
    assert_eq!(independent.state, DecisionState::Critical);
    assert!(independent.reason.contains("independent"));
}

#[test]
fn environmental_or_motion_signal_produces_watch_not_stable() {
    let mut engine = DecisionEngine::new(config()).unwrap();
    let result = engine.observe(Observation {
        environment_signal: true,
        ..observation(Some(0.1))
    });
    assert_eq!(result.state, DecisionState::Watch);
}

#[test]
fn config_rejects_invalid_threshold_order_and_zero_windows() {
    assert!(
        DecisionEngine::new(DecisionConfig {
            warning_threshold: 0.8,
            critical_threshold: 0.5,
            ..config()
        })
        .is_err()
    );
    assert!(
        DecisionEngine::new(DecisionConfig {
            confirmation_windows: 0,
            ..config()
        })
        .is_err()
    );
}

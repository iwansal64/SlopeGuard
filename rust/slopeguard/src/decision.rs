//! Conservative decision state machine for model output and sensor health.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionState {
    Stable,
    Watch,
    MovementRisk,
    Critical,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct DecisionConfig {
    pub warning_threshold: f64,
    pub critical_threshold: f64,
    pub exit_threshold: f64,
    pub confirmation_windows: u32,
    pub clear_windows: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Observation {
    pub risk_score: Option<f64>,
    pub data_ok: bool,
    pub environment_signal: bool,
    pub motion_signal: bool,
    pub independent_movement: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub state: DecisionState,
    pub reason: &'static str,
}

pub struct DecisionEngine {
    config: DecisionConfig,
    state: DecisionState,
    warning_count: u32,
    critical_count: u32,
    clear_count: u32,
}

impl DecisionEngine {
    pub fn new(config: DecisionConfig) -> Result<Self, &'static str> {
        if !config.warning_threshold.is_finite()
            || !config.critical_threshold.is_finite()
            || !config.exit_threshold.is_finite()
            || !(0.0..=1.0).contains(&config.exit_threshold)
            || !(0.0..=1.0).contains(&config.warning_threshold)
            || !(0.0..=1.0).contains(&config.critical_threshold)
        {
            return Err("thresholds must be finite and within 0..=1");
        }
        if config.warning_threshold >= config.critical_threshold {
            return Err("warning threshold must be below critical threshold");
        }
        if config.exit_threshold >= config.warning_threshold {
            return Err("exit threshold must be below warning threshold");
        }
        if config.confirmation_windows == 0 || config.clear_windows == 0 {
            return Err("confirmation and clear windows must be positive");
        }
        Ok(Self {
            config,
            state: DecisionState::Stable,
            warning_count: 0,
            critical_count: 0,
            clear_count: 0,
        })
    }

    pub fn state(&self) -> DecisionState {
        self.state
    }

    pub fn observe(&mut self, observation: Observation) -> Decision {
        if !observation.data_ok {
            self.reset_counts();
            self.state = DecisionState::Unknown;
            return Decision {
                state: self.state,
                reason: "required data stale or faulted",
            };
        }

        let Some(score) = observation.risk_score.filter(|score| score.is_finite()) else {
            self.reset_counts();
            self.state = DecisionState::Unknown;
            return Decision {
                state: self.state,
                reason: "risk score missing or non-finite",
            };
        };

        if observation.independent_movement {
            self.reset_counts();
            self.state = DecisionState::Critical;
            return Decision {
                state: self.state,
                reason: "independent movement evidence",
            };
        }

        match self.state {
            DecisionState::Unknown => {
                self.state = DecisionState::Stable;
                self.reset_counts();
            }
            DecisionState::Stable | DecisionState::Watch => {}
            DecisionState::MovementRisk => {
                if score < self.config.exit_threshold {
                    self.clear_count = self.clear_count.saturating_add(1);
                    if self.clear_count >= self.config.clear_windows {
                        self.state = DecisionState::Stable;
                        self.clear_count = 0;
                        self.warning_count = 0;
                        self.critical_count = 0;
                        return Decision {
                            state: self.state,
                            reason: "risk cleared after hysteresis",
                        };
                    }
                    return Decision {
                        state: self.state,
                        reason: "risk below exit threshold; hysteresis active",
                    };
                }
                self.clear_count = 0;
                if score < self.config.warning_threshold {
                    return Decision {
                        state: self.state,
                        reason: "risk remains above exit threshold; hysteresis active",
                    };
                }
            }
            DecisionState::Critical => {
                if score < self.config.exit_threshold {
                    self.clear_count = self.clear_count.saturating_add(1);
                    if self.clear_count >= self.config.clear_windows {
                        self.state = DecisionState::Stable;
                        self.clear_count = 0;
                        self.warning_count = 0;
                        self.critical_count = 0;
                        return Decision {
                            state: self.state,
                            reason: "risk cleared after hysteresis",
                        };
                    }
                    return Decision {
                        state: self.state,
                        reason: "critical risk below exit threshold; hysteresis active",
                    };
                }
                self.clear_count = 0;
                if score < self.config.critical_threshold {
                    return Decision {
                        state: self.state,
                        reason: "critical state held by hysteresis",
                    };
                }
            }
        }

        if score >= self.config.critical_threshold {
            self.critical_count = self.critical_count.saturating_add(1);
            self.warning_count = 0;
            if self.critical_count >= self.config.confirmation_windows {
                self.state = DecisionState::Critical;
                return Decision {
                    state: self.state,
                    reason: "critical risk confirmed",
                };
            }
            self.state = DecisionState::MovementRisk;
            return Decision {
                state: self.state,
                reason: "critical risk awaiting confirmation",
            };
        }

        self.critical_count = 0;
        if score >= self.config.warning_threshold {
            self.warning_count = self.warning_count.saturating_add(1);
            if self.warning_count >= self.config.confirmation_windows {
                self.state = DecisionState::MovementRisk;
                return Decision {
                    state: self.state,
                    reason: "movement risk confirmed",
                };
            }
            self.state = DecisionState::Watch;
            return Decision {
                state: self.state,
                reason: "movement risk awaiting confirmation",
            };
        }

        self.warning_count = 0;
        self.state = if observation.environment_signal || observation.motion_signal {
            DecisionState::Watch
        } else {
            DecisionState::Stable
        };
        Decision {
            state: self.state,
            reason: if self.state == DecisionState::Watch {
                "environment or motion signal present"
            } else {
                "no abnormal signal"
            },
        }
    }

    fn reset_counts(&mut self) {
        self.warning_count = 0;
        self.critical_count = 0;
        self.clear_count = 0;
    }
}

//! Advisory 30-minute forecast of reference-corrected pitch.
//!
//! Damped-trend (Holt) extrapolation of the causal as-of-joined pitch series,
//! plus a bounded environmental-loading offset. Pure function, no I/O.
//!
//! ponytail: this is an advisory extrapolation, not a validated predictor. It
//! never feeds `decide()` and claims no field accuracy. Swap the estimator for
//! a fitted model once independent displacement labels exist.

use serde_json::{Value, json};

pub(super) const STEP_S: i64 = 60;

const MIN_SAMPLES: usize = 15;
const MIN_SPAN_S: i64 = 600;
const TREND_WINDOW_S: i64 = 900;
const Z_BAND: f64 = 1.96;
const BAND_FLOOR_DEG: f64 = 0.01;
const RANGE_MULT: f64 = 3.0;
const RANGE_FLOOR_DEG: f64 = 0.5;
const LOAD_OFFSET_DEG: f64 = 0.5;
const RAIN_EXPECTED_BONUS: f64 = 0.5;
const REL_WARN_DEG: f64 = 2.0;
const RATE_WARN_DEG_MIN: f64 = 0.2;
const DISCLAIMER: &str =
    "Advisory extrapolation, not validated against displacement ground truth; never feeds alerts.";

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn empty(horizon_minutes: i64, quality: &str, reasons: Vec<&'static str>) -> Value {
    json!({
        "basis": "damped-trend+v1",
        "horizon_minutes": horizon_minutes,
        "step_s": STEP_S,
        "quality": quality,
        "disclaimer": DISCLAIMER,
        "degraded_reasons": reasons,
        "values": [],
    })
}

fn assess(pitch: f64, rate: f64) -> (&'static str, f64) {
    let p = pitch.abs();
    let r = rate.abs();
    let risk =
        (0.5 * (p / REL_WARN_DEG).min(1.0) + 0.5 * (r / RATE_WARN_DEG_MIN).min(1.0)).min(1.0);
    let state = if p >= REL_WARN_DEG && r >= RATE_WARN_DEG_MIN {
        "MOVEMENT_RISK"
    } else if p >= REL_WARN_DEG || r >= RATE_WARN_DEG_MIN {
        "WATCH"
    } else {
        "STABLE"
    };
    (state, risk)
}

/// `samples` are `(unix_ts, relative_pitch_deg)` pairs, ascending. `loading` is
/// a 0..1 environmental-loading index from the caller's rain/soil evidence.
pub(super) fn build(
    now: i64,
    samples: &[(i64, f64)],
    loading: f64,
    rain_expected: bool,
    rule_quality: &str,
    horizon_minutes: i64,
) -> Value {
    if rule_quality == "MISSING" {
        return empty(horizon_minutes, "MISSING", vec!["NO_DATA"]);
    }
    if rule_quality != "GOOD" {
        return empty(horizon_minutes, "DEGRADED", vec!["STALE_REFERENCE"]);
    }

    let window: Vec<(f64, f64)> = samples
        .iter()
        .filter(|(t, _)| *t <= now && now - *t <= TREND_WINDOW_S)
        .map(|(t, y)| ((*t - now) as f64 / 60.0, *y))
        .collect();
    let span_s = match (window.first(), window.last()) {
        (Some((first, _)), Some((last, _))) => (last - first) * 60.0,
        _ => 0.0,
    };
    if window.len() < MIN_SAMPLES || span_s < MIN_SPAN_S as f64 {
        return empty(horizon_minutes, "DEGRADED", vec!["SHORT_HISTORY"]);
    }

    let n = window.len() as f64;
    let sx: f64 = window.iter().map(|(x, _)| *x).sum();
    let sy: f64 = window.iter().map(|(_, y)| *y).sum();
    let sxx: f64 = window.iter().map(|(x, _)| x * x).sum();
    let sxy: f64 = window.iter().map(|(x, y)| x * y).sum();
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-9 {
        return empty(horizon_minutes, "DEGRADED", vec!["FLAT_HISTORY"]);
    }
    let slope = (n * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / n;

    let residuals: Vec<f64> = window
        .iter()
        .map(|(x, y)| y - (intercept + slope * x))
        .collect();
    let dof = (n - 2.0).max(1.0);
    let sigma = (residuals.iter().map(|r| r * r).sum::<f64>() / dof).sqrt();
    let den: f64 = residuals.iter().map(|r| r * r).sum();
    let num: f64 = residuals.windows(2).map(|w| w[0] * w[1]).sum();
    let phi = if den > 1e-12 {
        (num / den).clamp(0.5, 0.95)
    } else {
        0.85
    };

    let mut obs_min = f64::INFINITY;
    let mut obs_max = f64::NEG_INFINITY;
    for (_, y) in &window {
        obs_min = obs_min.min(*y);
        obs_max = obs_max.max(*y);
    }
    let range = (obs_max - obs_min).max(RANGE_FLOOR_DEG);
    let cap_lo = obs_min - RANGE_MULT * range;
    let cap_hi = obs_max + RANGE_MULT * range;

    let mut load = loading.clamp(0.0, 1.0);
    if rain_expected {
        load = (load + RAIN_EXPECTED_BONUS).min(1.0);
    }
    // Weather only amplifies an existing upward movement; it never invents one.
    let weather_offset = if slope > 0.0 {
        LOAD_OFFSET_DEG * load * (slope / RATE_WARN_DEG_MIN).min(1.0)
    } else {
        0.0
    };

    let mut values = Vec::with_capacity(horizon_minutes.max(0) as usize);
    for h in 1..=horizon_minutes {
        let hf = h as f64;
        let geo = phi * (1.0 - phi.powf(hf)) / (1.0 - phi);
        let pitch =
            (intercept + slope * geo + weather_offset * (1.0 - phi.powf(hf))).clamp(cap_lo, cap_hi);
        let band = Z_BAND * (sigma + BAND_FLOOR_DEG) * hf.sqrt();
        let rate = slope * phi.powf(hf);
        let (state, risk) = assess(pitch, rate);
        values.push(json!({
            "t_plus_min": h,
            "pitch_deg": r3(pitch),
            "band_deg": r3(band),
            "rate_deg_min": r3(rate),
            "state": state,
            "risk": r3(risk),
        }));
    }

    json!({
        "basis": "damped-trend+v1",
        "horizon_minutes": horizon_minutes,
        "step_s": STEP_S,
        "quality": "GOOD",
        "disclaimer": DISCLAIMER,
        "samples": window.len(),
        "span_s": r3(span_s),
        "slope_deg_min": r3(slope),
        "sigma_deg": r3(sigma),
        "phi": r3(phi),
        "loading": r3(load),
        "degraded_reasons": [],
        "values": values,
    })
}

/// True when any upcoming BMKG slot names rain. The numeric `tp` field is
/// undocumented, so only the reviewed text labels are trusted here.
pub(super) fn rain_expected(weather: &Value) -> bool {
    weather["slots"].as_array().is_some_and(|slots| {
        slots.iter().any(|slot| {
            ["condition_id", "condition_en"].iter().any(|key| {
                slot[*key].as_str().is_some_and(|text| {
                    let text = text.to_ascii_lowercase();
                    text.contains("hujan") || text.contains("rain")
                })
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(step: i64, n: i64, f: impl Fn(f64) -> f64) -> (i64, Vec<(i64, f64)>) {
        let start = 1_000_000;
        let samples = (0..n)
            .map(|i| {
                let minutes = i as f64 * step as f64 / 60.0;
                (start + step * i, f(minutes))
            })
            .collect();
        (start + step * (n - 1), samples)
    }

    #[test]
    fn constant_history_is_flat_with_widening_band() {
        let (now, s) = series(60, 21, |_| 0.5);
        let f = build(now, &s, 0.0, false, "GOOD", 30);
        assert_eq!(f["quality"], "GOOD");
        let vals = f["values"].as_array().unwrap();
        assert_eq!(vals.len(), 30);
        for v in vals {
            assert!((v["pitch_deg"].as_f64().unwrap() - 0.5).abs() < 0.01);
            assert_eq!(v["state"], "STABLE");
        }
        let first = vals[0]["band_deg"].as_f64().unwrap();
        let last = vals[29]["band_deg"].as_f64().unwrap();
        assert!(last > first, "band must widen: {first} -> {last}");
    }

    #[test]
    fn upward_ramp_is_damped_below_linear() {
        let (now, s) = series(60, 21, |min| 0.05 * min);
        let f = build(now, &s, 0.0, false, "GOOD", 30);
        let vals = f["values"].as_array().unwrap();
        let first = vals[0]["pitch_deg"].as_f64().unwrap();
        let last = vals[29]["pitch_deg"].as_f64().unwrap();
        // Linear 30-minute extrapolation from the last sample would be ~2.5 deg.
        assert!(last > 1.0 && last < 2.0, "damped value {last}");
        assert!(last > first);
        let r0 = vals[0]["rate_deg_min"].as_f64().unwrap();
        let r1 = vals[29]["rate_deg_min"].as_f64().unwrap();
        assert!(r1 < r0, "rate must decay with horizon");
    }

    #[test]
    fn short_history_degrades_without_values() {
        let (now, s) = series(60, 5, |min| min);
        let f = build(now, &s, 0.0, false, "GOOD", 30);
        assert_eq!(f["quality"], "DEGRADED");
        assert_eq!(f["degraded_reasons"], json!(["SHORT_HISTORY"]));
        assert_eq!(f["values"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn missing_or_stale_rule_quality_is_empty() {
        let (now, s) = series(60, 21, |min| 0.05 * min);
        assert_eq!(
            build(now, &s, 0.0, false, "MISSING", 30)["quality"],
            "MISSING"
        );
        assert_eq!(
            build(now, &s, 0.0, false, "DEGRADED", 30)["quality"],
            "DEGRADED"
        );
    }

    #[test]
    fn weather_loading_only_worsens_an_upward_trend() {
        let (now, up) = series(60, 21, |min| 0.05 * min);
        let dry = build(now, &up, 0.0, false, "GOOD", 30);
        let wet = build(now, &up, 1.0, true, "GOOD", 30);
        let dry_last = dry["values"][29]["pitch_deg"].as_f64().unwrap();
        let wet_last = wet["values"][29]["pitch_deg"].as_f64().unwrap();
        assert!(wet_last > dry_last);

        let (now, down) = series(60, 21, |min| -0.05 * min);
        let a = build(now, &down, 0.0, false, "GOOD", 30);
        let b = build(now, &down, 1.0, true, "GOOD", 30);
        assert_eq!(a["values"][29]["pitch_deg"], b["values"][29]["pitch_deg"]);
    }

    #[test]
    fn rain_expected_reads_forecast_text() {
        assert!(rain_expected(
            &json!({"slots": [{"condition_en": "Light Rain"}]})
        ));
        assert!(rain_expected(
            &json!({"slots": [{"condition_id": "Hujan Ringan"}]})
        ));
        assert!(!rain_expected(
            &json!({"slots": [{"condition_en": "Cloudy"}]})
        ));
        assert!(!rain_expected(&json!({"status": "disabled"})));
    }
}

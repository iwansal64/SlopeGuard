SLOPEGUARD ANALYSIS CONTRACT

Purpose

Step 3 converts accepted raw telemetry into causal feature rows, independent labels, chronological event splits, and transparent baseline predictions. Raw SQLite remains authoritative. CSV files are reproducible exchange artifacts, not a second telemetry store.

Scope

- One site, one movement node, one reference node.
- Prediction horizon: 30 minutes.
- Prediction time: t.
- Feature rule: every input packet has ts_device <= t.
- Ground truth: independent displacement evidence; never future tilt reused as a label.
- Synthetic replay proves plumbing only. It is not field validation.

Feature construction

Command:

  slopeguard features --db data/raw/slopeguard.db --out data/processed/features.csv

One output row is emitted per accepted movement-node packet. Reference data is an as-of join: the newest reference packet for the same site with ts_device <= t.

Windows:

- rain_15m_mm: movement-node rain sum in [t-15 min, t].
- rain_30m_mm: movement-node rain sum in [t-30 min, t].
- soil_moisture_pct: current value only.
- soil_rate_pct_min: current value versus the previous movement-node value.
- tilt_rate_deg_min: reference-corrected pitch versus the previous movement-node row.
- vibration_mag_g: mean absolute acceleration-magnitude deviation from 1 g in the causal 15-minute history.

Reference quality:

- reference_age_s is retained for every as-of reference.
- Reference-corrected tilt is usable only when sensor_status=OK and age <= 60 seconds.
- Older, faulted, or absent reference data sets reference_missing=true and leaves relative tilt/rate missing.
- Missing values remain empty. No optional sensor is zero-filled.

Label construction

Inputs:

- features CSV with the frozen FEATURE_HEADER;
- event CSV: event_id,start_ts,end_ts,scenario;
- independent truth CSV: event_id,ts_device,displacement_mm,evidence_file,review_status.

Command:

  slopeguard labels --features data/processed/features.csv --events data/experiments/events.csv --truth data/raw/ground_truth.csv --threshold-mm 1.0 --out data/labels/labels.csv

For a feature at t, reviewed truth points are searched only in (t, t+30 min] and within that event. movement_event=1 iff any displacement_mm >= the predeclared threshold. movement_event=0 is emitted only when the horizon is complete and reviewed evidence exists without a crossing. Unusable rows receive movement_event=-1 with label_status=INSUFFICIENT_HORIZON or NO_INDEPENDENT_EVIDENCE. Current-time truth is excluded.

Split manifest

Command:

  slopeguard split --labels data/labels/labels.csv --out data/validation/split_manifest.csv

Events, not rows, are sorted by earliest timestamp. Complete events are assigned chronologically: earliest 60% train, next 20% validation, latest 20% test. At least three events are required. One event cannot appear in more than one split.

Baselines

Command:

  slopeguard baselines --labels data/labels/labels.csv --split data/validation/split_manifest.csv --out data/validation/baseline_results.csv

Untuned implementation defaults, frozen before any model fitting:

- B0: positive when rain_15m_mm >= 5.0 and soil_moisture_pct >= 75.0.
- B1: positive when abs(tilt_rate_deg_min) >= 0.2 degrees/minute.
- B2: positive when B0 environmental loading and either B1 motion or abs(relative_pitch_deg) >= 2.0 degrees.

The output records baseline, event, split, actual label, prediction, score, and reason. It does not claim accuracy. Thresholds must be justified or selected on training/validation events before final test evaluation.

Validation boundary

Required before reporting competition metrics:

- real controlled-rig packets replacing synthetic replay;
- independent camera/marker/ruler or separate displacement reference;
- at least three complete experiments for event-wise splitting;
- frozen threshold, event IDs, provenance, and review status;
- baseline results on untouched test events.

Current verified evidence: unit/integration tests cover causal windows, stale reference handling, independent future labels, malformed CSV rejection, event-wise chronological splits, baseline output, and CLI feature generation. No field metric is claimed.

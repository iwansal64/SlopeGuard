SLOPEGUARD METRICS CONTRACT

Purpose

Step 6 evaluates frozen baseline and model predictions at row and event level. It prevents stable-row accuracy from hiding missed movement events and prevents synthetic replay from becoming a false competition claim.

Command

  slopeguard metrics \
    --labels data/labels/labels.csv \
    --split data/validation/split_manifest.csv \
    --baselines data/validation/baseline_results.csv \
    --models data/validation/model_results.csv \
    --out data/validation/metrics.csv \
    --report data/validation/metrics_report.txt \
    --source-kind synthetic

`source-kind` is required conceptually even though the CLI defaults to `synthetic`. Use a truthful value such as `synthetic`, `controlled_rig`, or `real_controlled_rig`; do not use a field label for synthetic data.

Methods and splits

The command requires predictions for all six methods and all three frozen splits:

- B0, B1, B2;
- motion, environment, fused;
- train, validation, test.

It verifies prediction headers, binary labels, event split agreement, duplicate prediction keys, finite scores, and agreement with the independent labeled rows. Missing or contradictory artifacts fail the command.

Metrics

`metrics.csv` contains:

- row_count and event_count;
- positive_events, detected_events, missed_events, false_alert_events;
- event_recall = detected positive events / positive events;
- event_false_alert_rate = false-alert negative events / negative events;
- diagnostic row accuracy, precision, recall, F1;
- score mean squared error;
- claimable and source_kind flags.

Event outcomes aggregate rows within an event:

- an event is positive if any labeled row is positive;
- a positive event is detected if any row is predicted positive;
- a negative event is a false alert if any row is predicted positive.

This makes event recall and event false-alert rate the primary warning metrics. Row accuracy remains diagnostic only.

Claim gate

The report always records:

- `source_kind`;
- `claimable`;
- an explicit status.

Synthetic data emits `claimable=false` and `NOT_REPORTABLE_UNTIL_REAL_CONTROLLED_RIG`. A real controlled-rig source remains review-required until the team verifies independent evidence, provenance, frozen event IDs, label review, and untouched test events. The command does not certify field performance or legal compliance.

Required evidence before competition metrics are quoted

- real controlled-rig packets replacing the stable-dry fixture;
- independent camera/marker/ruler or separate displacement reference;
- reviewed event IDs and evidence files;
- event-wise chronological split frozen before test inspection;
- calibration, mounting, units, missingness, and sensor-fault logs;
- baseline and model outputs generated from the same frozen artifacts;
- limitations stating within-rig scope, event count, and no official-warning authority.

Reproducibility

The output is byte-deterministic for identical inputs, source_kind, and code version. The report is an artifact for proposal/video review, not a substitute for field validation.

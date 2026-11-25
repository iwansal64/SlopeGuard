SLOPEGUARD MODEL AND DECISION CONTRACT

Purpose

Step 4 adds a deterministic logistic-regression model, three input ablations, and a conservative decision state machine. Raw SQLite, the feature contract, independent labels, and event-wise split manifest remain authoritative.

Model command

  slopeguard models \
    --labels data/labels/labels.csv \
    --split data/validation/split_manifest.csv \
    --out data/validation/model_results.csv \
    --report data/validation/model_report.txt

The command fails rather than fitting when no labeled rows exist, a split is missing, a split is empty, or the training split contains only one class.

Fit boundary

- Only rows assigned to train fit the scaler and logistic-regression parameters.
- Missing numeric inputs use the corresponding train mean; missingness is not silently converted to zero.
- Each fitted feature is standardized using the train mean and population standard deviation. A constant feature uses scale 1.
- Batch gradient descent uses L2 regularization (lambda 0.01), learning rate 0.10, maximum 2,000 epochs, and a fixed gradient tolerance.
- No random initialization, shuffling, or external model library is used; repeated runs are byte-deterministic.
- The validation set selects the threshold from 0.00..1.00 in 0.01 increments using F2 (beta=2). Ties prefer higher recall, then the lower threshold.
- The selected threshold is frozen before test evaluation. Test labels do not affect fitting, scaling, threshold selection, or model parameters.

Ablations

- motion: relative pitch/roll, tilt rate, vibration magnitude, acceleration magnitude;
- environment: 15/30-minute rainfall, soil moisture, soil-moisture rate;
- fused: all motion and environment features.

The result CSV contains train, validation, and test predictions for every variant. The report records feature lists, scaler parameters, model parameters, selected threshold, confusion cells, accuracy, precision, recall, F1, F2, specificity, balanced accuracy, and Brier score per split. These are evaluation artifacts, not field-performance claims.

Decision engine

The decision engine consumes a risk score plus data-quality and independent-movement flags. It emits only the frozen state vocabulary:

- STABLE: no abnormal signal;
- WATCH: environmental or motion signal, or risk awaiting confirmation;
- MOVEMENT_RISK: risk threshold confirmed;
- CRITICAL: critical risk threshold confirmed or independent movement evidence exists;
- UNKNOWN: required data is stale/faulted or the score is missing/non-finite.

Controls tested in `tests/decision.rs`:

- separate warning and critical thresholds;
- consecutive-window confirmation;
- lower exit threshold and consecutive-window hysteresis;
- immediate CRITICAL for independent movement evidence;
- UNKNOWN on stale, faulted, missing, or non-finite model input;
- invalid threshold/window configuration rejection.

Current implementation boundary

Step 4 provides the model CLI and decision module. Dashboard wiring, live model loading, actuator output, and real-rig evaluation remain later artifacts. No competition metric is reported from `stable_dry.jsonl`; it proves plumbing only. Real controlled-rig packets and independently reviewed displacement evidence are required before reporting performance.

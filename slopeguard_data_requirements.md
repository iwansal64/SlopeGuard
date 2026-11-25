SLOPEGUARD — DATA REQUIREMENTS

Objective

Predict whether an independently observed movement criterion will be crossed at one monitored slope within one fixed horizon, initially 30 minutes. Use “landslide” only if the experiments and labels justify that claim.

Core rule

Rows are not movement events. Thousands of samples from one stable experiment do not replace independent rainfall/movement experiments.

1. P0 DATA — REQUIRED FOR A CREDIBLE CORE

A. Raw motion telemetry

One record per node and sampling interval:

- timestamp_device;
- timestamp_server;
- site_id;
- node_id;
- node_role: movement/reference;
- accel_x_g, accel_y_g, accel_z_g;
- gyro_x_dps, gyro_y_dps, gyro_z_dps, if available;
- computed_pitch_deg and computed_roll_deg;
- sequence number;
- sensor_status;
- battery_voltage;
- signal strength;
- firmware_version;
- calibration_version.

Preserve raw axes and derived tilt. Never overwrite raw telemetry.

B. Environmental telemetry

- rain_mm_interval or rain_tip_count;
- rainfall accumulation over declared windows;
- soil_moisture_raw;
- soil_moisture_percent only after calibration;
- probe_depth_cm;
- probe_location/node association;
- optional temperature;
- environmental sensor status.

C. Installation and calibration metadata

One record per node/configuration:

- site_id;
- node_id and role;
- location/position on the slope or rig;
- mounting depth and geometry;
- axis orientation;
- reference-node location;
- baseline pitch and roll;
- calibration date/method;
- soil-probe depth and calibration method;
- rain-sensor calibration method;
- sampling interval and units;
- firmware and calibration version;
- missing-data policy.

A sensor fixed loosely in soil can measure enclosure movement or settling. Mounting details are part of the data, not decoration.

D. Independent ground-truth data

This is mandatory for a strong claim. Capture a source separate from the model input:

- camera timestamp and file ID;
- visible marker coordinates or ruler readings;
- independent displacement estimate;
- reference-frame description;
- manual review status;
- reviewer and review timestamp;
- measurement uncertainty;
- event ID.

Possible label sources:

- camera/marker measurement;
- separate displacement sensor;
- manually reviewed controlled experiment;
- field observation, if safely and legitimately collected;
- clearly labeled synthetic replay.

Do not use the same tilt value as both the only model input and the only ground-truth label.

E. Movement labels

For each prediction time t:

- prediction_timestamp;
- horizon_minutes;
- current independent displacement;
- max independent future displacement;
- movement_event: 0/1;
- movement_threshold and unit;
- crossing_timestamp, if positive;
- event_id;
- label_source;
- label_confidence;
- split: train/validation/test.

Prototype rule:

movement_event = 1 when the predeclared independent displacement criterion is crossed within the selected horizon. The criterion must be fixed before final evaluation.

F. Experiment/event records

For each controlled run:

- event_id;
- run ID and date;
- rig/site ID;
- initial soil condition;
- rainfall protocol and duration;
- water volume/intensity measurement, if controlled;
- sensor placement;
- operator;
- start/end timestamp;
- stable/movement/fault scenario;
- independent evidence file;
- maximum displacement;
- label decision and reviewer;
- train/validation/test assignment.

2. P1 DATA — HIGH VALUE AFTER P0 WORKS

A. Multiple motion nodes

- second moving-slope node;
- relative tilt between nodes;
- spatial gradient of movement;
- node agreement/disagreement;
- node-specific health.

Multiple nodes are valuable only when installation geometry and independent labels are documented.

B. BMKG/weather context

Optional until access is verified:

- station_id;
- station location and distance;
- observation timestamp and timezone;
- rainfall/precipitation;
- temperature;
- relative humidity;
- pressure, if available;
- units and aggregation interval;
- retrieval timestamp;
- source URL/API or file ID;
- missingness and quality flags.

Use BMKG history as contextual input, not as a substitute for local movement labels. Do not make the live model depend on an unverified external feed.

C. Human response/outcome logs

- alert_id;
- alert timestamp;
- risk state;
- acknowledgement timestamp;
- operator role;
- inspection/action taken;
- observed outcome;
- false-alert/true-alert status;
- notes.

These measure operational value. They are not training labels unless separately reviewed.

3. EXPERIMENT MATRIX

Minimum controlled dataset:

| Scenario | Rain | Soil | Ground movement | Purpose |
|---|---|---|---|---|
| Stable dry | low/none | low | none | control |
| Stable wet | increased | high | none | avoid wetness-only false alarms |
| Rainfall response | increased | rising | none or below threshold | measure environmental precursor |
| Controlled movement | declared | declared | independent criterion crossed | positive label |
| Sensor disturbance | any | any | no independent movement | fault detection |
| Network loss | any | any | any | fallback/recovery |

Repeat each important scenario. A single positive run is not strong validation.

4. FEATURE TABLE

Build only from values available before prediction time:

Motion

- current pitch/roll;
- change from baseline;
- tilt rate over 1/5/15/30 minutes;
- acceleration magnitude;
- vibration/variance statistics;
- movement-node minus reference-node angle;
- inter-node disagreement.

Environment

- rainfall over 5/15/30/60 minutes;
- soil-moisture level;
- soil-moisture rate of change;
- optional temperature/weather context.

Quality

- missingness flags;
- stale-data age;
- battery;
- signal;
- calibration version;
- reference agreement.

Do not forward-fill future values. Preserve an imputation/missingness flag if imputation is used.

5. DATASET SPLIT

Split by complete experiment/event, not random rows:

- training events: model fitting;
- validation events: threshold/model selection;
- test events: untouched final evaluation.

Chronological order is preferred. If only one rig exists, state that the test measures within-rig replay generalization, not field generalization.

Never put adjacent samples from one movement event into both training and test sets.

6. DATA QUALITY CHECKS

Before modeling:

- parse timestamps with explicit timezone;
- detect duplicates and out-of-order packets;
- check sequence gaps;
- check physical ranges;
- verify accelerometer magnitude around expected gravity during stationary periods;
- verify tilt repeatability at known orientations;
- verify reference-node stability;
- check rain accumulation/reset behavior;
- calibrate soil moisture against documented conditions;
- report sensor drift;
- retain raw files;
- record every dropped/imputed row;
- verify labels against independent evidence;
- freeze final test event IDs before model selection.

7. BASELINES AND ABLATIONS

Baselines:

- B0: rainfall + soil-moisture threshold;
- B1: tilt-rate threshold;
- B2: hand-designed sensor-fusion rule;
- B3: logistic regression;
- B4: tree model only if justified.

Ablations:

- motion only;
- environmental only;
- motion + environment;
- with and without reference-node correction;
- with and without verified BMKG context.

The selected AI must improve a user-relevant metric at a declared false-alert budget. If it does not, use the transparent rule and report that result.

8. REQUIRED METRICS

Report:

- event recall;
- missed-event rate;
- false alerts per experiment/day;
- warning lead time;
- precision/recall or PR-AUC;
- calibration/uncertainty limitation;
- inference latency;
- packet delivery rate;
- sensor uptime;
- tilt measurement repeatability;
- baseline versus AI.

Do not report row-level accuracy as the headline metric. It can be inflated by stable rows and hide missed movement events.

9. RECOMMENDED FILE LAYOUT

`data/raw/imu/` — immutable axes and gyro packets
`data/raw/environment/` — rain and soil packets
`data/raw/ground_truth/` — camera, marker, ruler, or independent displacement evidence
`data/metadata/` — node roles, mounting, calibration, units
`data/experiments/` — run protocols and event records
`data/processed/` — aligned feature tables
`data/labels/` — label-generation outputs and review records
`data/replays/` — deterministic demo scenarios
`data/validation/` — split manifests and metric outputs

Each directory needs a README with source, owner, collection method, timezone, units, missingness, permission/license status, and limitations.

10. DATA MINIMUM FOR THE COMPETITION DEMO

Required:

- stable IMU/reference telemetry;
- rainfall and soil-moisture telemetry;
- calibration metadata;
- stable and movement scenarios;
- independent movement evidence;
- sensor-fault and network-loss scenarios;
- label-generation script;
- threshold baseline;
- one AI result;
- provenance for every row;
- deterministic replay.

Synthetic/replay data can demonstrate system integration. It cannot be presented as field accuracy. The guidebook requires transparent explanation of primary, secondary, or synthetic data sources and permits a technically accountable prototype or simulation.[2]

11. FIRST DATA ARTIFACTS

Member 1 — `data_dictionary.md`

- field;
- meaning;
- unit;
- source;
- owner;
- allowed range;
- missing policy;
- label rule.

Member 2 — `device_samples.jsonl`

- 20 consecutive packets;
- movement/reference roles;
- sensor-fault log;
- network-loss log;
- calibration record.

Member 3 — `controlled_experiments.csv`, `labels.csv`, `replay.csv`, `baseline_results.csv`

- event timeline;
- independent evidence reference;
- labels;
- baseline output.

Pass condition

Another member can run one command and reproduce:

raw packet → calibrated feature → independent label → baseline prediction → AI prediction → saved metric

12. HIGHEST-LEVERAGE GAP

Independent ground truth. More sensors cannot compensate for labels that are circular, missing, or all generated from the same tilt signal.

Secure the controlled experiment and independent displacement reference before tuning the model.

Sources

[1] https://makerspace.binus.edu/binus-robotics-cup-2026 — BINUS Robotics Cup 2026 official announcement.
[2] file:///home/akirametakunai/IT/Competitions/AIoT_Binus/(v4.1)%20Guidebook%20AIoT.pdf — AIoT Innovation Competition Guidebook v4.1.
[3] https://doi.org/10.5194/adgeo-44-79-2017 — rainfall thresholds, soil-moisture modeling, and hydrohazard early warning.
[4] https://doi.org/10.1088/1742-6596/2312/1/012056 — low-cost accelerometer as inclinometer for soil-mass movement monitoring.
[5] https://doi.org/10.1088/1755-1315/873/1/012024 — MEMS inclinometer for landslide monitoring.
[6] https://doi.org/10.1016/j.catena.2021.105147 — antecedent soil moisture and rainfall thresholds for landslide early warning.

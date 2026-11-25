SLOPEGUARD — REAL-WORLD DEVICE JOURNEY

Purpose

Detect abnormal movement in one monitored slope and warn a human operator when movement risk rises. The prototype targets rainfall-induced shallow slope movement. It does not claim to predict every landslide or replace official authorities.

1. FIELD INSTALLATION AND COMMISSIONING

Prototype site
- One controlled soil-slope rig or safe test site.
- No intentional destabilization of an occupied or public slope.
- One primary movement node mounted rigidly into the slope/test mass.
- One stable reference node mounted to a rigid, non-moving base.
- Rain sensor above the test area.
- Soil-moisture probe at a documented depth.
- Independent camera/marker/ruler for ground-truth displacement.

Technician records
- site_id and node_id;
- node position and mounting depth;
- sensor orientation and axis convention;
- reference-node location;
- rainfall/soil-probe location and depth;
- calibration timestamp and firmware version;
- baseline pitch and roll;
- movement threshold used for labels;
- sampling interval and units.

The server accepts telemetry only from registered node_id/site_id pairs.

2. DEVICE SAMPLING LOOP

Every sampling cycle:

1. Read accelerometer axes.
2. Read gyroscope axes if available; use it for short dynamic-motion features, not unbounded angle integration.
3. Read rainfall and soil moisture.
4. Read battery, signal, uptime, and sensor status.
5. Apply range and stale-data checks.
6. Estimate pitch and roll relative to the calibrated baseline.
7. Compare movement node against the stable reference.
8. Store raw samples locally.
9. Publish telemetry or queue it during network loss.

Prototype sampling target:

- collect raw IMU samples at a rate justified by the hardware test;
- transmit compact aggregates at a fixed interval;
- retain raw anomaly windows for later inspection.

Do not silently replace missing sensor values with zero.

Illustrative telemetry payload:

{
  "site_id": "SLOPE-001",
  "node_id": "IMU-MOVE-01",
  "timestamp": "2026-11-25T08:30:00+07:00",
  "accel_x_g": 0.012,
  "accel_y_g": 0.684,
  "accel_z_g": 0.729,
  "gyro_x_dps": 0.08,
  "gyro_y_dps": -0.03,
  "gyro_z_dps": 0.02,
  "tilt_pitch_deg": 1.84,
  "tilt_roll_deg": -0.41,
  "rain_mm_interval": 3.2,
  "soil_moisture_pct": 68.4,
  "battery_v": 4.01,
  "signal_dbm": -76,
  "sensor_status": "OK",
  "firmware": "0.1.0"
}

The values above are interface examples, not experimental results.

3. LOCAL VALIDATION AND FALLBACK

The device checks:

- impossible acceleration magnitude;
- sensor timeout;
- excessive tilt jump;
- stale reference node;
- soil/rain sensor range;
- battery and queue capacity;
- timestamp and sequence continuity.

Local fallback:

- tilt-rate threshold;
- rainfall/soil-moisture threshold;
- consecutive-reading confirmation;
- local buzzer/LED;
- FALLBACK status when the server or model is unavailable.

A local fallback is not described as AI confidence.

4. CONNECTIVITY AND SERVER INGESTION

Path:

movement/reference nodes → MQTT or HTTPS → authentication → schema validation → raw event store → time-series storage

Server actions:

- authenticate node credentials;
- reject unknown devices;
- reject malformed, duplicate, stale, or impossible records;
- preserve raw payloads;
- attach server ingestion time;
- mark missingness and sensor quality;
- expose latest site health to the dashboard.

Every inference record remains traceable to site_id, node_id, timestamps, firmware, calibration version, and model version.

5. FEATURE BUILDING

For each prediction cycle, the server constructs only features available at prediction time:

Motion features
- pitch and roll;
- change from calibrated baseline;
- tilt rate over 1/5/15/30 minutes;
- acceleration magnitude and variability;
- short dynamic-motion statistics;
- movement-node minus reference-node angle;
- disagreement between multiple movement nodes, if available.

Environmental features
- rainfall over 5/15/30/60 minutes;
- soil moisture and rate of change;
- optional temperature;
- optional verified historical weather context.

Health features
- missingness flags;
- sensor age/staleness;
- battery;
- signal quality;
- firmware/calibration version.

No future tilt or future independent displacement enters the feature window.

6. GROUND-TRUTH LABELING

At prediction time t:

- record the current sensor features;
- observe the independent camera/marker/ruler reference after t;
- mark movement_event = 1 only when the predeclared independent displacement criterion is crossed within the chosen horizon;
- store the crossing timestamp and event_id;
- mark stable otherwise.

Do not generate labels from the same tilt measurement used as the only model input. That would make evaluation circular.

The label source must state whether it is:

- independent physical reference;
- manually reviewed video;
- controlled simulation;
- field observation;
- synthetic replay.

7. BASELINE AND ML PROCESSING

Baseline path

- rainfall threshold;
- soil-moisture threshold;
- tilt-rate threshold;
- reference-node consistency check.

AI path

- logistic regression first;
- gradient-boosted tree only if it improves the declared validation metric;
- output movement-risk probability/status, not an official warning level.

Target

P(movement criterion is crossed within the next H minutes)

Initial H: 30 minutes. Change only after data supports another horizon.

Illustrative inference result:

{
  "site_id": "SLOPE-001",
  "prediction_time": "2026-11-25T08:30:05+07:00",
  "horizon_minutes": 30,
  "risk_probability": 0.78,
  "risk_state": "MOVEMENT_RISK",
  "confidence": "MEDIUM",
  "model_version": "slopeguard-logreg-0.1",
  "input_quality": "GOOD",
  "reason_codes": ["TILT_RATE_RISING", "SOIL_MOISTURE_RISING"]
}

The numbers above are illustrative interface values, not measured results.

Required evaluation

- event recall;
- missed-event rate;
- false alerts per experiment/day;
- warning lead time;
- precision/recall or PR-AUC;
- tilt repeatability;
- packet delivery rate;
- inference latency;
- sensor uptime;
- baseline versus AI;
- motion-only versus environmental-only versus fused ablation.

8. DECISION AND ALERT ENGINE

Suggested states; thresholds require calibration:

STABLE
- no validated abnormal movement signal.

WATCH
- rainfall/soil wetness or tilt trend is increasing.

MOVEMENT RISK
- model or validated rule exceeds the warning condition for consecutive readings.

CRITICAL
- independent movement criterion is crossed or high-risk conditions persist.

UNKNOWN/DEGRADED
- required sensor/reference/data quality is missing or inconsistent.

Controls:

- consecutive readings before escalation;
- hysteresis before clearing;
- reference-node agreement check;
- visible confidence and sensor health;
- stale-data banner;
- human acknowledgement;
- no automatic evacuation command.

9. USER DASHBOARD

Top card
- STABLE/WATCH/MOVEMENT RISK/CRITICAL/UNKNOWN;
- probability or rule status;
- prediction horizon;
- confidence/data quality;
- last update age;
- model and calibration version.

Evidence panel
- movement-node tilt;
- reference-node tilt;
- relative tilt;
- tilt-rate chart;
- rainfall bars;
- soil-moisture trend;
- independent camera/marker evidence for replay;
- baseline beside AI;
- reason codes.

Operations panel
- node status;
- battery and signal;
- packet age and loss;
- alert acknowledgement;
- incident notes;
- local fallback status;
- recalibration/maintenance flag.

The dashboard must answer:

1. Is movement risk increasing?
2. How soon is the predicted window?
3. Can the measurement be trusted now?

10. HUMAN ACTION AND FEEDBACK

Example event:

08:10 — Rain starts.
08:15 — Soil moisture rises.
08:20 — Movement node begins diverging from the stable reference.
08:21 — Server receives valid telemetry.
08:21 — AI risk becomes MOVEMENT RISK.
08:22 — Dashboard and local buzzer activate.
08:23 — Operator acknowledges and inspects the test slope/site.
08:35 — Independent reference confirms movement or stable outcome.
After the event — Outcome, alert status, and notes are logged.

Operator acknowledgement is feedback for evaluation. It is not an automatic training label and does not trigger automatic retraining.

11. FAILURE JOURNEYS

Network loss

- Device stores a bounded local queue.
- Local threshold fallback continues.
- Local buzzer/LED remains available.
- Dashboard shows STALE/FALLBACK, never NORMAL by default.
- Queued telemetry uploads after reconnection.

Server/model outage

- Device continues local fallback.
- Dashboard shows service degradation.
- Alert carries FALLBACK, not AI confidence.

IMU fault or loose mount

- Range and stability checks flag the node.
- Reference disagreement lowers confidence.
- System returns UNKNOWN/DEGRADED.
- Recalibration/maintenance task is generated.

Soil/rain sensor fault

- Invalid feature excluded.
- Missingness flag retained.
- Risk state becomes degraded unless validated remaining signals suffice.

Low battery

- Maintenance alert generated.
- Last update age displayed.
- No claim that the site is safe after telemetry disappears.

12. COMPETITION GOLDEN PATH

Use one live node plus deterministic replay:

1. Show calibrated stable readings.
2. Replay rainfall increase.
3. Show soil-moisture response.
4. Show relative tilt increase against the reference node.
5. Show telemetry ingestion.
6. Show feature values.
7. Show baseline and AI outputs.
8. Trigger dashboard warning and local buzzer.
9. Show independent displacement evidence.
10. Disconnect the network.
11. Show local fallback and STALE/FALLBACK dashboard state.
12. Reconnect and show queued-data recovery.
13. Display measured metrics and limitations.

Do not depend on real landslide creation during judging.

13. MINIMUM SYSTEM BOUNDARY

Build now:

- one primary movement node;
- one stable reference;
- rainfall sensor;
- soil-moisture sensor;
- independent displacement reference;
- local fallback and alert;
- one transport;
- raw event storage;
- one baseline;
- one ML model;
- one dashboard;
- one replayable experiment.

Defer:

- four hazards;
- regional prediction;
- autonomous evacuation;
- deep learning;
- automatic retraining;
- multi-site fleet;
- pore-pressure instrumentation unless already available;
- mobile app.

14. SCORE CONNECTION

Proposal: define the geotechnical boundary, labels, independent ground truth, baseline, leakage prevention, calibration, safety, and limitations.

Video: show the complete sensor-to-warning loop, independent movement evidence, and network fallback.

Final: demonstrate a repeatable controlled experiment, not a theatrical claim of a real landslide.

The highest-leverage evidence is independent ground truth showing that fused AI warning improves on threshold rules.

Sources

[1] https://makerspace.binus.edu/binus-robotics-cup-2026 — BINUS Robotics Cup 2026 official announcement.
[2] file:///home/akirametakunai/IT/Competitions/AIoT_Binus/(v4.1)%20Guidebook%20AIoT.pdf — AIoT Innovation Competition Guidebook v4.1.
[3] https://doi.org/10.5194/adgeo-44-79-2017 — rainfall thresholds, soil-moisture modeling, and hydrohazard early warning.
[4] https://doi.org/10.1088/1742-6596/2312/1/012056 — low-cost accelerometer as inclinometer for soil-mass movement monitoring.
[5] https://doi.org/10.1088/1755-1315/873/1/012024 — MEMS inclinometer for landslide monitoring.
[6] https://doi.org/10.1016/j.catena.2021.105147 — antecedent soil moisture and rainfall thresholds for landslide early warning.

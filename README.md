# SlopeGuard

## AIoT Early Warning for Rainfall-Induced Slope Movement

Proposed project for the BINUS Robotics Cup 2026 AIoT Innovation Competition.

Chosen subtheme:

> AIoT untuk Ketahanan Lingkungan & Mitigasi Iklim

Status:

> Prototype/research proposal. Not an official landslide-warning authority.

## 1. Executive summary

SlopeGuard monitors a small, instrumented slope and estimates whether abnormal ground movement is likely within a defined near-term horizon. It combines:

- accelerometer-based tilt/inclinometer measurements;
- optional gyroscope measurements for short dynamic motion;
- rainfall;
- soil moisture;
- sensor-health and connectivity data;
- optional verified BMKG weather context.

The output is an uncertainty-aware action state for a human operator:

- STABLE;
- WATCH;
- MOVEMENT RISK;
- CRITICAL;
- UNKNOWN/DEGRADED.

The initial claim is deliberately narrow:

> SlopeGuard detects and warns about abnormal movement in a tested slope setup; it does not claim to predict every landslide in Indonesia.

The core research question:

> Does sensor fusion of tilt change, rainfall, and soil moisture improve near-term slope-movement warning over transparent threshold rules?

## 2. Why this fits the competition

The guidebook explicitly describes AIoT for disaster risk using sensor fusion, machine learning, and real-time adaptive early warning.[2] SlopeGuard provides the required functional loop:

```text
Slope sensors
    |
    v
ESP32 node + local validation + local fallback
    |
    v
HTTPS connectivity (TLS terminated by reverse proxy)
    |
    v
Rust receiver + feature builder + baseline + ML model
    |
    v
Movement-risk decision and explanation
    |
    v
Dashboard + local buzzer/LED + human acknowledgement
    |
    v
Independent movement outcome and event log
```

The guidebook permits a physical prototype or technically accountable simulation, provided the AI and IoT integration is demonstrable.[2]

## 3. Problem and user

### Target user

A village disaster-response team, local government operator, site manager, or trained slope-monitoring volunteer.

### Operational problem

Rainfall and soil wetness can increase concern, but the operator also needs evidence that the ground is actually changing. A warning system that shows only weather data cannot distinguish a wet but stable slope from a slope beginning to move.

### Decision

```text
Should the operator inspect the slope, notify the responsible authority, or escalate the local response checklist now?
```

SlopeGuard supports that decision. It does not authorize evacuation or replace geotechnical assessment.

## 4. Prediction contract

Freeze these values before model training.

| Item | Initial contract |
|---|---|
| Pilot | One controlled slope/test site |
| Primary hazard | Rainfall-induced shallow slope movement |
| Forecast horizon | 30 minutes initially; change only with evidence |
| Target event | Independent displacement/ground-movement threshold crossed within the horizon |
| Inputs | Tilt change/rate, rainfall, soil moisture, sensor health |
| Output | Risk state, probability, horizon, reason codes, confidence/status |
| Action | Inspect, acknowledge, and follow a human response checklist |
| Ground truth | Camera/marker/ruler or separate displacement reference; never the same input signal alone |
| Safety boundary | Decision support; no official warning or evacuation claim |

Use “slope-movement early warning” in the prototype title unless independent labels justify the stronger word “landslide.”

## 5. Device design

### Minimum prototype

- ESP32-class controller;
- one primary MEMS accelerometer/IMU node fixed to the test slope;
- one stable reference node or rigid reference fixture;
- rain gauge/sensor;
- capacitive soil-moisture sensor;
- local buzzer and status LED;
- battery/power monitor;
- Wi-Fi/4G connectivity for the prototype;
- independent camera, marker, ruler, or displacement reference for labels.

### Sensor interpretation

An accelerometer can estimate static tilt from gravity after calibration. A gyroscope measures angular rate and drifts when integrated; it is optional for short dynamic-motion features, not the sole long-term inclinometer. The mounting structure matters as much as the sensor: a loose enclosure can report its own movement rather than slope movement.

Use at least one stable reference. Add a second moving-slope node only after the single-node path works.

## 6. AI design

### Features

- pitch and roll relative to the calibrated baseline;
- tilt rate and recent change;
- acceleration magnitude and vibration statistics;
- difference between movement node and reference node;
- rainfall accumulation over 5/15/30/60 minutes;
- soil-moisture level and rate of change;
- temperature, if it affects sensor stability;
- missing, stale, battery, and connectivity flags.

### Baselines

- B0: rainfall and soil-moisture thresholds;
- B1: tilt-rate threshold;
- B2: hand-designed fusion rule;
- B3: logistic regression;
- B4: gradient-boosted tree only if it improves the declared metric.

The AI model must beat the strongest transparent baseline at the same false-alert budget, or the simpler baseline remains the honest result.

### Labels

At prediction time t, label the sample positive only when an independently measured displacement/ground-movement threshold is crossed during the next H minutes. Features may use only information available at t. Do not use the future tilt signal to create the label for a tilt-based model.

## 7. Data and validation

The highest-risk dependency is ground truth. Build a controlled soil-slope rig rather than intentionally destabilizing a real slope.

Required scenarios:

1. Stable dry control.
2. Stable wet control.
3. Rainfall without movement.
4. Gradual controlled movement with independent displacement evidence.
5. Sensor disturbance or loose-mount fault.
6. Network loss and recovery.

Use event-wise chronological splits. Rows from one rainfall experiment must not appear in both training and test sets.

Report:

- event recall;
- missed-event rate;
- false alerts per day or per experiment;
- warning lead time;
- precision/recall or PR-AUC;
- tilt repeatability;
- packet delivery rate;
- sensor uptime;
- inference latency;
- baseline versus AI;
- ablation: tilt only, environmental only, fused inputs.

Published systems support rainfall/soil-moisture thresholding for hydrohazard warning, while studies describe low-cost accelerometer/MEMS inclinometer approaches for movement monitoring.[3][4][5][6]

## 8. Server and dashboard

Path:

```text
sensor payload → authenticated ingestion → raw store → feature windows → model/baseline → decision engine → dashboard/alert
```

Dashboard must show:

- risk state;
- probability/status;
- prediction horizon;
- tilt and tilt-rate plots;
- rainfall and soil-moisture plots;
- movement/reference-node comparison;
- reason codes;
- last update age;
- sensor health and battery;
- baseline versus AI result;
- alert acknowledgement;
- fallback/stale-data state.

The dashboard must answer:

1. Is movement risk increasing?
2. How soon is the predicted window?
3. Can the system be trusted now?

### Demonstrate BMKG forecasts without hardware

From the repo root, rebuild the dashboard, then run it on a free local port:

```sh
cargo build --locked --release --manifest-path rust/dashboard/Cargo.toml
SG_DASH_ADDR=127.0.0.1:18091 ./run.sh dash
```

Open `http://127.0.0.1:18091/`, expand **Set device location**, enter the
pilot village/ward's `adm4` code (for example `31.71.03.1001` for Kemayoran),
and save. The dashboard writes `config/site.json` (git-ignored), reloads the
forecast immediately, and keeps the location across restarts and database
resets. `SG_BMKG_ADM4` remains a one-shot fallback when that file is absent.
No API key or receiver is needed for the forecast. The dashboard stays
loopback-only, so configure it from the machine running it; a phone needs a
LAN-exposed, authenticated deployment first (not implemented).

Location is site configuration, not telemetry. Packets carry `site_id` but no
GPS or coordinates. Sending a location in every packet would change the frozen
schema for data that never changes; sending it only in the first packet loses
it on packet loss, replay, or reboot, and duplicate suppression can discard
the retry. A server-side record is retryable, idempotent, survives reboots,
and leaves the device firmware unchanged. The ESP hotspot can carry the
configuration request, but the dashboard host still needs internet for BMKG.

[BMKG's public forecast](https://data.bmkg.go.id/prakiraan-cuaca/) provides
area forecasts at three-hour intervals. The dashboard shows up to three
upcoming forecast times, conditions, temperature, and air humidity. It
refreshes every 30 minutes after success, retries after five minutes on
failure, and marks remaining cached forecasts as stale if a refresh fails.
`GET /api/data` exposes the same snapshot under `weather`, including
source, area code, forecast issue time per slot, and fetch timestamp when
available. Each slot also carries BMKG's undocumented `tp` value as
`precipitation`, unlabelled: confirm its unit before any prediction use.
Offsets such as `+0700` are normalized to an IANA zone so the browser can
render local time. `POST /api/site` accepts `{"adm4":"..."}` only from the
dashboard's own origin and validates the code before writing the file.
Missing readings are unknown, not zero. Forecasts do not enter the sensor
log, animation, model, or slope verdict; wiring them into prediction is later
work. With no sensor data, the slope verdict remains `UNKNOWN`. The synthetic
replay still works offline: `python3 scripts/demo.py storm`.

## 9. Golden-path demonstration

1. Show calibrated stable readings.
2. Apply controlled rainfall.
3. Show soil moisture rising.
4. Show tilt change from the movement node relative to the stable reference.
5. Send telemetry to the server.
6. Show feature construction.
7. Compare baseline and AI output.
8. Trigger MOVEMENT RISK and local buzzer/LED.
9. Show independent marker/camera movement evidence.
10. Disconnect the network.
11. Show local fallback and dashboard STALE/DEGRADED.
12. Reconnect and show queued-data recovery.
13. Display measured metrics and limitations.

Use a deterministic replay backup. Do not depend on creating a real landslide during judging.

## 10. Failure and safety controls

| Failure | Required behavior |
|---|---|
| Loose or shifted sensor mount | Detect reference disagreement; show degraded status; require recalibration |
| Accelerometer/IMU fault | Reject impossible/stale values; do not substitute zero |
| Soil sensor fault | Exclude invalid feature; lower confidence |
| Network loss | Continue local threshold fallback; buffer bounded queue |
| Server/model outage | Continue local fallback; show FALLBACK, not AI confidence |
| Low battery | Show maintenance warning; preserve last known state honestly |
| Unknown model input | Return UNKNOWN/DEGRADED; no false high-confidence alert |
| Real-slope deployment | Use trained operator, conservative limits, and no autonomous evacuation action |

These are design controls and test targets, not certification or legal-compliance claims.

## 11. Originality position

Do not present the work as:

> IMU + rainfall sensor + Random Forest + dashboard.

Present it as:

> A reference-corrected, uncertainty-aware AIoT early-warning loop that distinguishes wet-but-stable conditions from measurable slope movement, then preserves a local fallback when connectivity or the model fails.

The defensible contribution is the decision loop:

- environmental conditions indicate loading;
- multiple/reference-corrected motion measurements indicate actual change;
- AI fuses the signals at a defined horizon;
- the system exposes confidence and sensor health;
- an operator acknowledges the alert and records the outcome.

## 12. Scope cuts

Do not build before the core passes:

- four-hazard prediction;
- nationwide landslide prediction;
- deep learning;
- automatic retraining;
- autonomous evacuation or earthmoving;
- complex geotechnical digital twin;
- mobile app;
- multi-site fleet;
- expensive pore-pressure instrumentation unless already available.

One repeatable slope-movement experiment beats a broad disaster dashboard.

## 13. Team ownership

### Member 1 — Proposal/research

Problem definition, literature, data provenance, labels, validation, limitations, proposal, video, presentation, Q&A. Must run the full demo.

### Member 2 — Device/embedded

IMU mounting and calibration, reference node, rainfall/soil sensors, firmware, local queue, health telemetry, buzzer/LED, power, failure tests.

### Member 3 — Software/ML

Ingestion, storage, features, baseline, model, decision engine, dashboard, deterministic replay, metrics, offline fallback.

Every member must explain the full device → server → model → dashboard loop.

## 14. Competition deliverables

The guidebook states:

- Full proposal: 15–20 pages, PDF maximum 10 MB;
- Harvard Style citations;
- similarity result maximum 20%;
- prototype/simulation video: MP4, 3–7 minutes, minimum 720p, 16:9;
- final presentation: 12 minutes plus 8 minutes Q&A;
- final scoring: research depth 30%, technical AIoT implementation 30%, innovation/originality and impact 20%, presentation/Q&A 20%.[2]

The official webpage lists registration through 30 November 2026 and the Creative Innovation final window as 14–17 December 2026; the guidebook gives its own schedule. Confirm any discrepancy with the organizer before submission.[1][2]

## 15. Immediate go/no-go experiment

Build this first:

```text
controlled rainfall
    → soil moisture response
    → tilt change relative to reference
    → independent displacement label
    → server inference
    → warning
```

Pass condition:

- stable control remains stable;
- controlled movement is repeatable;
- tilt change precedes independent movement evidence;
- sensor-fault and network-loss behavior are visible;
- five complete demo runs succeed.

If this fails, narrow the claim to “slope-movement monitoring” and do not claim landslide prediction.

## 16. Project files

- `slopeguard_device_journey.md` — real-world device-to-dashboard journey;
- `slopeguard_build_roadmap.md` — six-day build and submission plan;
- `slopeguard_data_requirements.md` — telemetry, labels, experiments, and validation;
- `rust/` — the single implementation: contract, ingest, replay, HTTP receiver;
- `SlopeGuard-README-dark.pdf` — generated project brief;
- `(v4.1) Guidebook AIoT.pdf` — official local guidebook; do not edit.

## Sources

[1] https://makerspace.binus.edu/binus-robotics-cup-2026 — BINUS Robotics Cup 2026 official announcement.

[2] file:///home/akirametakunai/IT/Competitions/AIoT_Binus/(v4.1)%20Guidebook%20AIoT.pdf — AIoT Innovation Competition Guidebook v4.1.

[3] https://doi.org/10.5194/adgeo-44-79-2017 — rainfall thresholds, soil-moisture modeling, and hydrohazard early warning.

[4] https://doi.org/10.1088/1742-6596/2312/1/012056 — low-cost accelerometer used as an inclinometer for soil-mass movement monitoring.

[5] https://doi.org/10.1088/1755-1315/873/1/012024 — MEMS inclinometer for landslide monitoring.

[6] https://doi.org/10.1016/j.catena.2021.105147 — antecedent soil moisture and rainfall thresholds for landslide early warning.

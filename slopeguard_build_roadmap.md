SLOPEGUARD — 3-MEMBER BUILD ROADMAP

Mission

Ship one judge-proof landslide/slope-movement early-warning system:

slope sensors → connectivity → server → baseline + ML → dashboard/alert → local fallback → independent outcome

Use the narrower claim “rainfall-induced slope-movement early warning” until independent labels justify “landslide detection.”

CURRENT CONSTRAINT

Current working date: 25 November 2026 WIB.

Proposal/video deadline in the local guidebook: 30 November 2026, 23:59 WIB.[2]

Upload before the final evening. Freeze the submission package by 18:00 WIB on 30 November.

TEAM OWNERSHIP

Member 1 — Proposal and research lead

Owner:
- user/problem definition;
- literature and citations;
- data provenance;
- independent-label protocol;
- validation metrics;
- proposal and video script;
- limitations, risk, safety, and Q&A.

Constraint: not a passive writer. Must run the full device → server → model → dashboard demo.

Member 2 — Device and embedded lead

Owner:
- accelerometer/IMU mounting;
- stable reference node;
- sensor calibration;
- rainfall and soil-moisture sensors;
- firmware;
- local queue;
- connectivity;
- buzzer/LED;
- power and fault handling.

Constraint: document whether the sensor measures tilt, angular rate, or vibration. Do not call gyro integration a stable long-term inclinometer.

Member 3 — Software and ML lead

Owner:
- ingestion;
- raw/time-series storage;
- feature windows;
- baseline;
- model;
- decision engine;
- dashboard;
- deterministic replay;
- metrics and offline fallback.

Constraint: provide a reproducible command that another member can run without hardware.

SHARED RULES

- One owner per artifact.
- All three members can execute the golden path.
- End every work session with a 15-minute integration test.
- One transport: MQTT or HTTPS. Pick one.
- One storage path first. Do not build a platform.
- No unsupported “accuracy” claims.
- Every external claim has a citation; every measurement has provenance.

HARD SCOPE CUT

Build only:

- one controlled slope/test site;
- one primary movement node;
- one stable reference;
- rainfall;
- soil moisture;
- independent displacement reference;
- one warning horizon;
- one baseline;
- one AI model;
- one dashboard;
- one local fallback.

Defer:

- flood, typhoon, drought, and four-hazard prediction;
- nationwide or regional landslide prediction;
- deep learning;
- autonomous evacuation;
- automatic retraining;
- multi-site fleet;
- mobile app;
- expensive geotechnical sensors unless already available.

DEFINITION OF DONE

A fresh run must:

1. Read or replay timestamped IMU/environment data.
2. Show raw and calibrated sensor values.
3. Ingest and validate the payload.
4. Build features without future-data leakage.
5. Generate an independent movement label.
6. Produce baseline and AI outputs.
7. Show risk, horizon, reason codes, confidence, and health.
8. Trigger dashboard and local alert.
9. Continue with local fallback during network loss.
10. Recover queued data after reconnection.
11. Show measured metrics, provenance, and limitations.

GO/NO-GO TEST

Proceed with a landslide claim only if the controlled experiment shows:

- stable control remains stable;
- controlled movement is repeatable;
- tilt change precedes independent displacement evidence;
- labels do not come from the same input signal alone;
- sensor and network faults are visible;
- five end-to-end demonstrations succeed.

If this fails, use “slope-movement monitoring” and remove “landslide prediction” from the title and claims.

PHASE 0 — CONTRACT AND HARDWARE FREEZE
25 NOVEMBER — FIRST WORK SESSION

Member 1
- Freeze the prediction contract: movement event within 30 minutes.
- Define the user and response checklist.
- Define the independent movement label.
- Create source/claim/evidence ledger.
- Create proposal skeleton.

Member 2
- Inventory controller, IMU/accelerometer, optional gyro, rain sensor, soil probe, buzzer/LED, power, enclosure, reference fixture, camera.
- Decide mounting geometry.
- Mark primary node versus stable reference.
- Create a calibration worksheet.

Member 3
- Freeze telemetry JSON schema.
- Create repository and one local replay command.
- Choose MQTT or HTTPS.
- Define raw event and inference records.

Shared gate

Approve:
- one architecture;
- one target horizon;
- one independent label;
- one baseline;
- one demo story;
- one fallback.

Artifact: `docs/system_contract.md`

Pass condition: no sensor, feature, model, or screen lacks a decision role and owner.

PHASE 1 — DEVICE-TO-SERVER VERTICAL SLICE
25–26 NOVEMBER

Member 2
- Read IMU axes.
- Calibrate orientation at rest.
- Compute pitch/roll or equivalent tilt values.
- Read rain and soil moisture.
- Add timestamp, node/site ID, health, firmware.
- Add range checks and local queue.
- Trigger local fallback on declared threshold conditions.

Member 3
- Implement receiver.
- Validate schema, timestamps, sequence, and ranges.
- Persist raw packets unchanged.
- Expose latest node/reference health.
- Add replay fixture.
- Return ingestion acknowledgement.

Member 1
- Define field dictionary, units, calibration record, missing-data policy.
- Document accelerometer tilt versus gyroscope rate.
- Draft problem, objective, and limitation sections.

Integration gate

- One real or simulated packet reaches storage.
- Primary node and reference node can be compared.
- Malformed packet is rejected and logged.
- Network loss does not stop local alert behavior.
- 20 consecutive packets pass without silent failure.

Artifact: telemetry contract plus screen recording.

PHASE 2 — CONTROLLED SLOPE EXPERIMENT AND BASELINE
26–27 NOVEMBER

Member 2
- Build soil-slope test rig.
- Mark independent displacement reference.
- Run stable-dry, stable-wet, rainfall, and controlled-movement cases.
- Measure IMU repeatability at rest.
- Record mounting disturbance and sensor-fault behavior.
- Never destabilize an occupied/public slope.

Member 3
- Build aligned feature table.
- Create event IDs.
- Implement label-generation script from independent camera/marker/ruler data.
- Implement baseline: rainfall + soil moisture + tilt-rate thresholds.
- Produce replay CSV.

Member 1
- Create data dictionary and event log.
- Define label confidence and independent-evidence rules.
- Define event-wise chronological split.
- Write validation metrics and acceptance criteria.
- Draft methodology and testing sections.

Integration gate

- At least one stable control and one movement scenario are independently labeled.
- No future observations enter features.
- Baseline output is reproducible.
- All rows have provenance and event IDs.

Artifacts:
- `data_dictionary.md`;
- `controlled_experiments.csv`;
- `labels.csv`;
- `baseline_results.csv`;
- `validation_protocol.md`.

Pass condition: another member reproduces labels and baseline with one command.

PHASE 3 — AI MODEL AND DECISION ENGINE
27–28 NOVEMBER

Member 2
- Harden firmware: timeout, impossible-value rejection, queue bounds, reconnect, watchdog/reset.
- Test loose-mount/reference disagreement.
- Test network loss and recovery.
- Capture battery/signal/uptime.

Member 3
- Train logistic regression first.
- Compare a tree model only if validation supports it.
- Implement motion-only, environmental-only, and fused ablations.
- Implement STABLE, WATCH, MOVEMENT RISK, CRITICAL, UNKNOWN/DEGRADED.
- Add consecutive-reading confirmation and hysteresis.
- Expose model/calibration versions and reason codes.

Member 1
- Compare AI against baseline at the same false-alert budget.
- Report event recall, missed events, false alerts, warning lead time, PR-AUC/precision-recall, latency, uptime, and packet delivery.
- Write limitations: controlled rig, small event count, site specificity, sensor mounting, no official warning authority.
- Prepare evidence table for every headline claim.

Integration gate

- AI changes a measurable decision or the team reports no improvement.
- Unknown/degraded data cannot produce false high confidence.
- Labels remain independent from model inputs.
- Fixed replay gives deterministic output.

Artifacts:
- `model_report.md`;
- saved model;
- inference command/API;
- metric table;
- ablation table.

PHASE 4 — DASHBOARD AND GOLDEN PATH
28 NOVEMBER

Member 2
- Finish rigid mounts and cable strain relief.
- Label axes and calibration points.
- Add visible node/reference status.
- Prepare spare cables, power, and backup sensor footage.

Member 3
- Build one dashboard screen, not a platform.
- Show risk state, probability/status, horizon, confidence, tilt, relative tilt, rainfall, soil moisture, independent evidence, AI/baseline, health, and fallback.
- Add replay control.
- Add stale-data and model-unavailable banners.

Member 1
- Write 3–7 minute video script.
- Create architecture and data-flow diagrams.
- Draft proposal Chapters I–III, budget, schedule, output, safety, and limitations.
- Ensure title says slope movement unless go/no-go test passes.

Golden-path test

1. Stable calibrated state.
2. Rainfall replay.
3. Soil moisture rises.
4. Relative tilt increases.
5. Server receives payload.
6. Features appear.
7. Baseline and AI outputs appear.
8. MOVEMENT RISK appears.
9. Local buzzer/LED activates.
10. Independent displacement evidence appears.
11. Network disconnects.
12. Local fallback continues; dashboard shows STALE/FALLBACK.
13. Network restores; queued data uploads.

Artifact: working dashboard plus full recording.

Pass condition: five successful runs out of five.

PHASE 5 — VALIDATION, PROPOSAL, VIDEO, RED TEAM
29 NOVEMBER

Member 1
- Complete 15–20-page proposal.
- Keep PDF ≤10 MB.
- Use Harvard citations.
- Add similarity result and originality declaration.
- Insert measured results only.
- Add research gap, methodology, validation, budget, schedule, safety, scale, and limitations.
- Time the video.

Member 2
- Run 30–60-minute hardware stability test.
- Test sensor disconnect, loose mount, impossible values, power interruption, network interruption.
- Record recovery behavior.
- Prepare portable physical setup.

Member 3
- Run clean replay from a fresh environment.
- Export plots and metrics.
- Test restart of ingestion, storage, model, and dashboard.
- Package offline replay.
- Freeze model/calibration versions for the video.

Shared red-team questions

- Is this landslide detection or only slope movement?
- What independently proves movement?
- Why AI instead of thresholds?
- Why two/reference-corrected nodes?
- How is sensor settling distinguished from ground movement?
- How are labels created?
- How is leakage prevented?
- What happens when rain or soil data is missing?
- What happens when network/server fails?
- Who acts on the alert?
- What is original?
- What is not proven?

Pass condition: each answer has evidence, an assumption, or an explicit limitation.

PHASE 6 — SUBMISSION FREEZE
30 NOVEMBER — SUBMIT EARLY

Member 1
- Final proofread and citation check.
- Check 15–20 pages, PDF size, filename, similarity attachment, and public video link.
- Ensure proposal, video, code, and live demo agree.

Member 2
- Freeze firmware.
- Photograph final device.
- Save wiring/mounting diagram, calibration procedure, and firmware version.
- Do not add hardware features.

Member 3
- Freeze software/model/dashboard.
- Package replay, offline fallback, and exact run command.
- Save model/calibration IDs and version hashes.

Shared
- Upload before 18:00 WIB.
- Download submitted files.
- Verify PDF opens and video is accessible.
- Preserve the exact submitted versions.

Artifact: final proposal, public video, and submission receipt.

POST-SUBMISSION — FINAL PREPARATION
1–13 DECEMBER

Member 1
- Build 12-minute presentation.
- Lead research, evidence, impact, safety, limitations, and Q&A.

Member 2
- Practice physical setup, calibration, reset, power, sensor replacement, and local fallback.

Member 3
- Practice offline replay, dashboard, alert, network failure, and recovery.

All members
- Run 10 complete rehearsals.
- Rotate explanations.
- Keep a failure log.
- Remove fragile features.

FINAL PRESENTATION RUN OF SHOW

0:00–1:00 — User, slope risk, decision.
1:00–2:00 — Why rainfall-only warning is insufficient.
2:00–5:30 — Device → server → features → AI → dashboard demonstration.
5:30–7:00 — Independent ground truth and baseline comparison.
7:00–8:30 — Network/sensor failure fallback.
8:30–10:00 — Deployment, maintenance, safety, scale.
10:00–11:15 — Results and limitations.
11:15–12:00 — Original contribution and close.

TASK BOARD

P0 — Must ship

- IMU/tilt telemetry;
- stable reference;
- rainfall and soil telemetry;
- independent movement labels;
- server ingestion;
- baseline;
- one ML model;
- dashboard;
- local fallback;
- replay fixture;
- validation metrics;
- compliant proposal/video.

P1 — Add only after P0 passes

- second moving-slope node;
- verified BMKG context;
- camera overlay in dashboard;
- alert messaging;
- better enclosure;
- extra plots.

P2 — Do not build before competition

- four hazards;
- national prediction;
- autonomous evacuation;
- deep learning;
- automatic retraining;
- multi-site fleet;
- mobile application;
- pore-pressure hardware unless already available.

HIGHEST-LEVERAGE SCORING GAP

Independent ground truth. A sophisticated model without independent movement labels is weaker than a simple model with repeatable, leakage-free validation.

Next artifact:

`controlled_experiments.csv` + `labels.csv` + one end-to-end replay showing:

rainfall → soil moisture → relative tilt → independent displacement → baseline/AI → warning → fallback

Sources

[1] https://makerspace.binus.edu/binus-robotics-cup-2026 — BINUS Robotics Cup 2026 official announcement.
[2] file:///home/akirametakunai/IT/Competitions/AIoT_Binus/(v4.1)%20Guidebook%20AIoT.pdf — AIoT Innovation Competition Guidebook v4.1.
[3] https://doi.org/10.5194/adgeo-44-79-2017 — rainfall thresholds, soil-moisture modeling, and hydrohazard early warning.
[4] https://doi.org/10.1088/1742-6596/2312/1/012056 — low-cost accelerometer as inclinometer for soil-mass movement monitoring.
[5] https://doi.org/10.1088/1755-1315/873/1/012024 — MEMS inclinometer for landslide monitoring.
[6] https://doi.org/10.1016/j.catena.2021.105147 — antecedent soil moisture and rainfall thresholds for landslide early warning.

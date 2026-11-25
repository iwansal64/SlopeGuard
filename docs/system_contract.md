SYSTEM CONTRACT v1 — FROZEN 2026-11-25

Owner: member 3 (software/ML). Authoritative code: `rust/slopeguard/src/contract.rs`.
Change anything here only by incrementing `schema_version` and editing this file
and the Rust constant set in the same commit. There is no second implementation.

SCOPE

Software-only prototype. One site, one movement node, one stable reference
node, rainfall, soil moisture, independent displacement ground truth,
one 30-minute horizon, one baseline, one model, one dashboard, one fallback.

CLAIM

"Rainfall-induced slope-movement early warning." Not landslide prediction.
Not an official warning authority. No autonomous evacuation.

PREDICTION CONTRACT

  target          independent displacement criterion crossed within horizon
  horizon         30 minutes (change only with evidence)
  prediction time t   features may use only data at or before t
  ground truth    camera/marker/ruler or separate displacement reference,
                  never the tilt signal that feeds the model

DECISION STATES (output vocabulary)

  STABLE          no validated abnormal movement signal
  WATCH           environmental loading or tilt trend rising
  MOVEMENT_RISK   model/rule above warning condition, confirmed
  CRITICAL        independent movement criterion crossed, or high risk persists
  UNKNOWN         required sensor/reference data missing or inconsistent
                  (code constant: "UNKNOWN"; UI may render UNKNOWN/DEGRADED)

TRANSPORT — ONE ONLY

  HTTP/1.1 POST, JSON body = one telemetry packet,
  `Authorization: Bearer <node token>`. Implemented in
  `rust/slopeguard/src/receiver.rs` (`slopeguard serve`).
  Response: 202 on accept, 409 on duplicate, 422 on contract violation
  (body carries the reject reason), 401 on bad token or node/token mismatch,
  413 on body > 8 KiB, 404 unknown path, 405 wrong method. Request head capped
  at 16 KiB. GET /health returns the latest packet per node and also requires a
  token. No MQTT. TLS is terminated by a reverse proxy in deployment; the pilot
  listener is plain HTTP on 127.0.0.1 only.

TELEMETRY PACKET — schema_version 1

Required (16):
  schema_version     int, must equal 1
  site_id            str, ^[A-Z0-9][A-Z0-9_-]{2,31}$
  node_id            str, same pattern
  node_role          "movement" | "reference"
  seq                int >= 0, monotonic per (site_id, node_id)
  ts_device          RFC 3339 with explicit offset or Z (lowercase z allowed)
  accel_x_g           -2 .. 2
  accel_y_g           -2 .. 2
  accel_z_g           -2 .. 2
  tilt_pitch_deg      -90 .. 90
  tilt_roll_deg       -90 .. 90
  battery_v           3.0 .. 4.5
  signal_dbm          -120 .. 0
  sensor_status      "OK" | "FAULT" | "STALE"
  firmware           str, version pattern, <=32
  calibration_version  str, version pattern, <=32

Optional float fields (null = sensor absent; NEVER 0 as a stand-in):
  gyro_x_dps, gyro_y_dps, gyro_z_dps   -2000 .. 2000
  rain_mm_interval                     0 .. 50
  soil_moisture_raw                    0 .. 4095
  soil_moisture_pct                    0 .. 100
  probe_depth_cm                       0 .. 200
  temp_c                               -20 .. 60

Unknown fields are rejected: the packet IS the contract.

PHYSICAL CHECKS

  gravity    sqrt(ax^2+ay^2+az^2) in 0.3 .. 1.7 g. Outside = fault, reject.
  finite     NaN and +/-inf rejected (JSON permits them; the contract does not).
  tilt       tilt_*_deg is tilt RELATIVE TO THE CALIBRATED BASELINE, so it is
             not expected to equal atan2(accel). Do not derive-and-compare;
             the accel magnitude check is the fault signal.

INGEST POLICY

  malformed JSON   reject, log raw text + reason in `rejected`
  contract break   reject, log all error codes
  duplicate        same (site_id, node_id, seq) rejected as `duplicate:seq`;
                   re-running a replay is idempotent
  out of order     accepted (queued-recovery is legitimate), counted
  old timestamps   accepted; staleness is a query-time property, not an
                   ingest-time rejection
  raw payload      stored byte-for-byte in `raw_packet.payload`, never rewritten
  no packet is ever silently dropped: every ingest ends in raw_packet or rejected

STORAGE — ONE PATH

  SQLite, `data/raw/slopeguard.db` (WAL). Tables:
    raw_packet(id, site_id, node_id, seq, node_role, ts_device, received_at,
               sensor_status, battery_v, signal_dbm, payload,
               UNIQUE(site_id,node_id,seq))
    rejected(id, received_at, source, payload, reason)
  Latest accepted packet per node = node health, via `latest_health()`.

ANALYSIS LAYER — STEP 3 IMPLEMENTED

  Authoritative code: `rust/slopeguard/src/analysis.rs`.
  Features are causal: every packet used has `ts_device <= t`.
  Reference correction is an as-of join within the same site. A reference
  older than 60 seconds or not `OK` is exposed as stale/missing; its relative
  tilt is not used. Optional values remain missing, never zero-filled.
  Feature output: `data/processed/features.csv`.
  Independent labels use reviewed displacement points in `(t,t+30 min]` and
  the predeclared threshold. Current-time evidence is excluded. Incomplete
  horizon/evidence rows receive `movement_event=-1`, not a guessed negative.
  Split manifest is event-wise chronological (60% train, 20% validation, 20%
  test) and requires at least three events.  B0/B1/B2 are transparent baseline
  predictions; thresholds are implementation defaults, not reported results.
  Step 4 adds train-only logistic regression with motion/environment/fused
  ablations, validation threshold selection, untouched test output, and the
  conservative decision engine. Step 6 adds event-level metrics with an
  explicit synthetic-data claim gate. Full details: `docs/analysis_contract.md`,
  `docs/model_contract.md`, and `docs/metrics_contract.md`.

LABEL RULE (frozen before training)

  movement_event = 1 iff the predeclared independent displacement criterion
  (threshold, unit, and reference frame fixed before final evaluation) is
  crossed within 30 minutes after prediction time t.
  Splits are event-wise chronological; no event spans train and test.

RUN COMMAND

  cargo test --manifest-path rust/slopeguard/Cargo.toml
  rust/slopeguard/target/release/slopeguard replay --out data/replays/stable_dry.jsonl
  rust/slopeguard/target/release/slopeguard ingest data/replays/stable_dry.jsonl --db data/raw/slopeguard.db --reset
  rust/slopeguard/target/release/slopeguard serve --addr 127.0.0.1:8080 --tokens config/nodes.tokens

OPEN / DEFERRED

  - operator read-only token for /health
  - live model inference wiring into the dashboard
  - hardware firmware (out of software scope; member 2)
  - real rig data replacing the synthetic fixture before any competition metric is reported
  - final replay packaging and version freeze (step 6)

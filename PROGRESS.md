SLOPEGUARD — BUILD PROGRESS LOG

Single implementation: Rust only (`rust/slopeguard`). Dashboard is a separate
Rust crate (`rust/dashboard`) plus vanilla static assets (`web/`). Read this
first when resuming; do not re-decide anything listed under Frozen.

Frozen (do not change without a new schema_version + doc edit)
- One horizon: 30 min. One site. One movement node + one reference node.
- Claim = "rainfall-induced slope-movement early warning", never landslide prediction.
- States: STABLE, WATCH, MOVEMENT_RISK, CRITICAL, UNKNOWN.
- Transport: HTTP/1.1 POST JSON + per-node bearer token. No MQTT.
  TLS is terminated by a reverse proxy in deployment; pilot binds 127.0.0.1.
- Storage: one SQLite path, `data/raw/slopeguard.db`. Raw payload kept byte-for-byte.
- Contract lives in `rust/slopeguard/src/contract.rs` + `docs/system_contract.md`.
- No Python, no pip, no JS runtime. SlopeGuard crates: chrono, clap, rusqlite,
  serde_json only. Dashboard uses the same Rust dependencies plus a path dependency.

STEP MAP
  1  contract + validating ingest + raw store + deterministic replay + tests   DONE
  2  HTTP receiver (token auth, 202/409/422/401/413) + /health + live test     DONE
  3  feature windows + independent labels + event split + B0/B1/B2            DONE
  4  logistic regression + ablations + decision engine                         DONE
  5  dashboard + replay control + stale/fallback banners                       DONE
  6  metrics + replay packaging + freeze versions                               IN PROGRESS

STEP 1 — DONE 2026-11-25

  `contract.rs`, `ingest.rs`, `replay.rs`, `main.rs`, SQLite raw/rejected store,
  stable-dry fixture, deterministic replay, validation and regression tests.

STEP 2 — DONE 2026-11-25

  `receiver.rs`: stdlib HTTP/1.1 sequential receiver.
  POST `/telemetry`: 202 accept, 401 auth/mismatch, 409 duplicate, 413 body cap,
  422 JSON/contract violation, 500 store failure.
  GET `/health`: authenticated latest-node health.
  Constant-time token comparison, strict node-token mapping, request caps.

STEP 3 — DONE 2026-11-25

  `analysis.rs`: causal feature windows, same-site as-of reference join,
  60-second reference freshness, independent displacement labels, strict CSV
  headers/widths, event-wise chronological train/validation/test manifest,
  B0/B1/B2 transparent baselines.
  Docs: `docs/analysis_contract.md`.
  Synthetic fixture remains pipeline-only; no field metric reported.

STEP 4 — DONE 2026-11-25

  `model.rs`: deterministic logistic regression; train-only imputation,
  standardization, fitting; validation F2 threshold selection; untouched test
  evaluation; motion/environment/fused ablations; deterministic model report.
  `decision.rs`: confirmation windows, critical escalation, hysteresis,
  UNKNOWN on stale/faulted/non-finite input, immediate CRITICAL on independent
  movement evidence. Critical state does not downgrade on one intermediate score.
  CLI: `models`.
  Docs: `docs/model_contract.md`.

STEP 5 — DONE 2026-11-25

  `rust/dashboard`: read-only SQLite API, static-file server, real receiver
  replay endpoint, loopback guard, 10-second replay cooldown, stale/fallback
  banners. `web/`: single-screen scene, trend/loading charts, trust rail,
  scrubber, live mode, keyboard controls, responsive/reduced-motion states.
  Dashboard intentionally labels its current live display as evidence baseline;
  live model score is not yet wired into the UI.
  UI log: `UI_PLAN.md`.

STEP 6 — IN PROGRESS 2026-11-25

  `metrics.rs`: validates baseline/model artifacts, computes row and event
  diagnostics for B0/B1/B2/motion/environment/fused across train/validation/test,
  writes deterministic `metrics.csv` + `metrics_report.txt`, marks synthetic
  output `claimable=false` and `NOT_REPORTABLE_UNTIL_REAL_CONTROLLED_RIG`.
  CLI: `metrics`.
  Docs: `docs/metrics_contract.md`.
  Remaining: run real controlled-rig collection, generate real artifacts,
  package deterministic replay/version manifest, and freeze submission outputs.

Behavior
  - validate() returns sorted error codes: missing:/unknown:/bad:/range:/gravity/not_object.
  - Unknown fields rejected; bool is not an int; NaN/inf rejected; optional fields
    may be null, never zero-filled; null required field is missing:<key>.
  - tilt_*_deg is baseline-relative; it is not compared to atan2(accel).
    Fault detection = accel magnitude outside 0.3..1.7 g.
  - Duplicate (site,node,seq) -> rejected reason `duplicate:seq`; replay reruns idempotent.
    Out-of-order accepted + counted; old timestamps accepted; staleness is query-time.
  - Reject paths commit. CLI exit codes: 0 = something accepted, 1 = nothing accepted,
    2 = unreadable file.
  - Model test data does not become competition evidence. Metrics command requires
    truthful `source_kind` and reports claimability explicitly.

Verified (actual)
  - Official announcement fetched: `https://makerspace.binus.edu/binus-robotics-cup-2026/`.
    It states registration 10 Sep–30 Nov 2026, initial review 1–3 Dec, finalists
    4 Dec, technical meeting 5 Dec, final 14–17 Dec, award 18 Dec. Guidebook
    deadline remains 30 Nov 2026 23:59 WIB; official page was current at check time.
  - `cargo test --manifest-path rust/slopeguard/Cargo.toml` -> 7 unit +
    7 analysis + 4 CLI + 19 contract + 6 decision + 13 ingest + 2 metrics +
    2 model + 8 receiver + 7 replay = 75 tests passed, 0 failed.
  - `cargo clippy --manifest-path rust/slopeguard/Cargo.toml --all-targets -- -D warnings` -> clean.
  - `cargo fmt --manifest-path rust/slopeguard/Cargo.toml -- --check` -> clean.
  - `cargo build --manifest-path rust/slopeguard/Cargo.toml --release` -> success.
  - `cargo test --manifest-path rust/dashboard/Cargo.toml` -> 3 passed, 0 failed.
  - `cargo clippy --manifest-path rust/dashboard/Cargo.toml --all-targets -- -D warnings` -> clean.
  - `cargo fmt --manifest-path rust/dashboard/Cargo.toml -- --check` -> clean.
  - Live dashboard smoke with receiver + dashboard: initial UNKNOWN/no data;
    `/api/replay` stable -> `accepted=40`, `duplicate=0`, `rejected=0`; read-back
    -> 40 SQLite packets, 2 nodes, 20 movement + 20 reference series points,
    receiver up, STABLE evidence state.
  - Replayed same fixture again -> `accepted=0`, `duplicate=40`, `rejected=0`;
    read-back remained 40 packets.
  - Dashboard HTML served HTTP 200; web assets copied and served in smoke.

Known gaps / risks
  - No real controlled-rig data, independent reviewed displacement files, or
    reportable competition metric yet.
  - Live model inference is not wired into the dashboard; dashboard state remains
    explicitly labeled evidence baseline.
  - No committed final replay/model/version manifest yet.
  - `/health` has no separate read-only operator token; any node token can read it.
  - HTTPS termination is manual reverse proxy; no TLS code in-repo.
  - Dashboard has no auth and must remain loopback-only.
  - Official page and guidebook differ in some schedule wording; organizer confirmation
    remains required if a discrepancy affects submission.

Next single action
  Collect and commit one controlled-rig evidence bundle: `data/experiments/events.csv`,
  `data/raw/ground_truth.csv`, raw telemetry/replay, calibration/mount metadata, and
  review log. Pass condition: at least three complete event IDs, independent displacement
  evidence, stable/movement scenarios, provenance per file, then run features → labels →
  split → baselines → models → metrics with `--source-kind real_controlled_rig`; do not
  quote any metric until the resulting report passes human provenance review.

# SlopeGuard (Rust)

Single implementation of the SlopeGuard telemetry path: frozen contract,
validating ingest, deterministic replay, and the HTTP receiver. No Python, no
second language to keep in sync, no framework.

## Build, run, test

```sh
cargo build --release --manifest-path rust/slopeguard/Cargo.toml
cargo test --manifest-path rust/slopeguard/Cargo.toml
cargo clippy --manifest-path rust/slopeguard/Cargo.toml --all-targets -- -D warnings

rust/slopeguard/target/release/slopeguard replay --out data/replays/stable_dry.jsonl
rust/slopeguard/target/release/slopeguard ingest data/replays/stable_dry.jsonl --db data/raw/slopeguard.db --reset
rust/slopeguard/target/release/slopeguard serve --addr 127.0.0.1:8080 --db data/raw/slopeguard.db --tokens config/nodes.tokens
bash scripts/smoke_receiver.sh    # live HTTP end-to-end, from the repo root
```

`ingest` exit codes: 0 = something accepted, 1 = nothing accepted, 2 = unreadable
file. `serve` exit codes: 0 = clean stop, 1 = store/bind failure, 2 = unreadable
or invalid token file.

## Layout

- `slopeguard/src/contract.rs` — packet validation, frozen schema v1
- `slopeguard/src/ingest.rs` — SQLite store, ingest, rejects, node health
- `slopeguard/src/receiver.rs` — HTTP/1.1 receiver, per-node bearer tokens
- `slopeguard/src/replay.rs` — deterministic fixture generator
- `slopeguard/src/analysis.rs` — causal features, independent labels, splits, B0/B1/B2
- `slopeguard/src/main.rs` — CLI: `ingest`, `replay`, `serve`, `features`, `labels`, `split`, `baselines`
- `slopeguard/tests/` — contract, ingest, replay, analysis, CLI, receiver

## HTTP API

One packet per request. `Authorization: Bearer <token>`; the token file maps one
token to exactly one `node_id` (`config/nodes.tokens`, one `NODE_ID token` per
line, >= 16 chars). A packet whose `node_id` does not match the token's node is
`401`, with the same body as an unknown token so no detail leaks.

| Request | Responses |
|---|---|
| `POST /telemetry` (JSON body) | `202` accepted, `401` bad token/node, `409` duplicate `(site,node,seq)`, `413` body > 8 KiB, `422` malformed JSON or contract violation (body carries the reject reason), `500` store failure |
| `GET /health` (any valid token) | `200` `{"count":N,"nodes":[latest packet per node]}`, `401` |
| anything else | `404`; wrong method on a known path is `405` |

Request head is capped at 16 KiB and body at 8 KiB before `ingest_lines` runs.
Requests are handled sequentially: two nodes at one packet per 10 s do not need a
thread pool.

TLS is terminated by a reverse proxy in deployment. The pilot listener is plain
HTTP bound to `127.0.0.1`; never expose it directly. `/health` leaks node ids,
battery, and sensor status, so it requires a token too; a read-only operator
token is a deferred improvement.

## Contract decisions

- Timestamps are RFC 3339 only (offset or `Z`, lowercase `z` allowed). Space
  separators, missing seconds, basic format, comma fractions, hour-only offsets,
  ISO week dates, and `24:00` are rejected and logged with the raw payload.
- A required field present but `null` is `missing:<key>`, not a database error.
- `validate` returns sorted error codes: `missing:`, `unknown:`, `bad:`,
  `range:`, `gravity`, `not_object`.
- Unknown fields are rejected: the packet IS the contract.
- NaN/inf cannot appear in JSON text; they arrive as malformed JSON and are
  rejected before validation.
- `seq >= 2^63` is `bad:seq`; trailing newlines in ids are rejected.
- Malformed JSON text is logged as `json:<serde message>` with the raw payload.
- Every ingested line ends in `raw_packet` or `rejected`; nothing is silently
  dropped.

## Not built yet

Logistic regression, decision engine, and dashboard. Feature windows, independent
labels, event-wise split manifests, and B0/B1/B2 baseline outputs are implemented
in `analysis.rs`. The fixture is synthetic and proves the pipeline, not the slope:
no accuracy or lead-time claim may be reported until recorded rig data replaces it.

Analysis commands:

```sh
slopeguard features --db data/raw/slopeguard.db --out data/processed/features.csv
slopeguard labels --features data/processed/features.csv --events data/experiments/events.csv --truth data/raw/ground_truth.csv --out data/labels/labels.csv
slopeguard split --labels data/labels/labels.csv --out data/validation/split_manifest.csv
slopeguard baselines --labels data/labels/labels.csv --split data/validation/split_manifest.csv --out data/validation/baseline_results.csv
```

See `docs/analysis_contract.md` for headers, leakage rules, label policy, and validation boundaries.

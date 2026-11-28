# SlopeGuard UI — build plan and resume log

Owner of this file: UI workstream. The Rust backend workstream owns
`PROGRESS.md`; this file never overrides it.

## Status: STEP 8 of 9 partly done (2026-11-28)

Working dashboard at `http://127.0.0.1:8090`, one entry point at the repo root:

```sh
./run.sh demo      # build, start receiver + dashboard, foreground, Ctrl-C stops both
./run.sh test      # tests + clippy + fmt for both crates
./run.sh smoke     # live HTTP end-to-end (backend script)
./run.sh storm     # wrapper for python3 scripts/demo.py storm
./run.sh status    # ports, packet count, rule state, receiver health
./run.sh reset     # delete the database
./run.sh help      # everything else
```

`SG_DB`, `SG_TOKENS`, `SG_ADDR`, `SG_DASH_ADDR`, `SG_RATE` override the defaults.

## Frozen (do not re-decide)

- The backend is `rust/slopeguard`. UI work touches **no file** there.
  New crate `rust/dashboard`, static UI in `web/`.
- No Node, no npm, no build step, no browser dependency. Vanilla ES module +
  canvas. Total shipped: 8.9 KB HTML, 13.6 KB CSS, 22 KB JS (uncompressed).
- Ports: receiver `127.0.0.1:8080`, dashboard `127.0.0.1:8090`.
- The dashboard reads SQLite **read-only** and never writes telemetry.
- Replay is pushed through the real receiver (`POST /telemetry`) by
  `scripts/demo.py` with the node tokens read locally. Tokens never reach the
  browser, and the website has no demo UI.
- Fixtures have distinct seq ranges (storm 1..120, dry 5001..5020) so the calm
  control can follow the storm in one database; replaying either twice is a
  409 duplicate by design.
- State vocabulary is frozen: STABLE, WATCH, MOVEMENT_RISK, CRITICAL, UNKNOWN.
  The displayed state is an **evidence baseline** (thresholds copied from
  `analysis::BaselineConfig::default()`), labelled as such. No model score is
  invented; the model fact row says `not wired, rule shown`.
- Any state that is not GOOD data quality renders UNKNOWN plus a stale/fault
  banner. The dashboard never shows "normal" for a site it cannot hear.
- Synthetic fixtures are labelled synthetic in the rail.

## Step map

- [x] 0 Inventory: contract, receiver routes, DB schema, fixture, device journey
- [x] 1 Plan frozen (this file)
- [x] 2 Read-only API + static server (`GET /api/data`)
- [x] 3 UI shell: bar, slope scene, verdict, gauges, trust rail
- [x] 4 Charts: relative tilt vs 2.0° baseline; rain bars + soil line
- [x] 5 Time scrubber + live edge + keyboard control
- [x] 6 States: first run, no telemetry, stale, sensor fault, receiver down,
      replay failure, reduced motion, 390 px, 1536 px
- [x] 7 Verify: build, clippy, fmt, tests, live end-to-end, DOM audit
- [x] 8 Weather: BMKG forecast panel, operator-set device location, precipitation passthrough
- [ ] 9 Handoff note to the backend agent (below)

## What the screen says

Top-left, in order: the state word, one sentence, reason codes. The scene is a
site cross-section: the movement mast tilts by the measured relative angle
(×4, labelled), the reference mast sits on bedrock, rain falls when rain is
measured, the water table follows soil moisture. The gauges under it are the
four numbers a judge asks about. The rail answers the other three questions:
is risk rising (tilt trend vs baseline), what is loading the slope (rain, soil),
and can the measurement be trusted (packet age, battery, signal, rejects,
receiver state, model state). The strip scrubs the recording; markers show rain
onset and the 2° crossing.

Keyboard: `L` live, `←/→` step, `?` help.

## API contract (v0, owned by the UI crate)

`GET /api/data?window_s=900` (clamp 60..21600) returns one object:
`now`, `window_s`, `db{ok,path,bytes,packets,rejects,last_reject}`,
`nodes[{site_id,node_id,node_role,seq,ts_device,received_at,age_s,
sensor_status,battery_v,signal_dbm,packets}]`,
`rule{basis,horizon_minutes,state,reasons,quality,inputs,thresholds}`,
`model{available,report_present}`, `receiver{url,up}`,
`series{movement,reference}` where each series is
`{node_id, points:[[t,pitch,roll,rel_pitch,rel_roll,rain,soil,batt,rssi,seq,status]]}`.

Points are arrays, not objects (smaller payload, faster parse). `t` is epoch
seconds. Missing values are `null`, never 0.

Demo replay is terminal-only: `python3 scripts/demo.py storm|dry|stable`
pushes the fixture through the receiver. `--rate 1..60` and
`--receiver`/`--tokens` override the defaults.

`GET /api/data` also carries `weather{status,source,adm4,area,city,timezone,
fetched_at,slots[]}`, where `status` is `disabled|loading|ready|stale|
unavailable` and each slot is `{valid_at,issued_at,condition_id,condition_en,
temperature_c,humidity_pct,precipitation}`. `precipitation` is BMKG's
undocumented `tp`, passed through unlabelled.

`POST /api/site` body `{"adm4":"31.71.03.1001"}` validates the ward code, writes
`config/site.json` atomically, and restarts the BMKG snapshot for that area.
Same-origin and loopback checks match `/api/explain`; the file is git-ignored.

## Device location: one server-side record, not telemetry

Decision: the location is dashboard configuration keyed to the pilot site, not
a telemetry field. Telemetry has no GPS or coordinates, only `site_id`.

- Per-packet location: forces a schema-version change for a value that never
  changes, adds bytes to every packet, and needs a GPS fix (unreliable indoors).
- First-packet-only: one lost first packet, reboot, or replay leaves the site
  unlocated; duplicate suppression can discard the retry; backend restarts need
  persistence anyway.
- Server-side record: the operator saves once from the dashboard; the write is
  validated, atomic, idempotent, visible to every consumer, and survives
  restarts plus `./run.sh reset` (which deletes the database, not the file).

The ESP hotspot can expose the configuration page, but the dashboard currently
binds loopback only. A phone therefore cannot reach it until a LAN binding with
operator authentication exists. Configure from the machine running the
dashboard; an ESP captive portal would POST the same `/api/site` contract with a
provisioning token (future work). The dashboard host still needs internet for
BMKG; the hotspot alone does not provide it.

## Verified (actual)

```
cargo test  --manifest-path rust/dashboard/Cargo.toml          -> 13 passed
cargo clippy --manifest-path rust/dashboard/Cargo.toml --all-targets -D warnings -> clean
cargo fmt --manifest-path rust/dashboard/Cargo.toml -- --check -> clean
node scripts/smoke_weather.mjs                                 -> pass (copy, form IDs, states)
no config/site.json -> weather disabled, location form auto-open, table hidden
POST /api/site {"adm4":"31.71.03.1001"} -> 200, 3 forecast rows, rule UNKNOWN
restart with config on disk -> weather ready without env var
390x844 location form -> no overflow, input 292 px, button 56x32
cargo test --manifest-path rust/slopeguard/Cargo.toml          -> all binaries pass
./run.sh demo (SG_DB=/tmp/..., alt ports)                      -> page 200
python3 scripts/demo.py storm                                  -> accepted 240, MOVEMENT_RISK
python3 scripts/demo.py storm (again)                          -> duplicate 240 (idempotent)
./run.sh test                                                  -> 78 tests pass, clippy and fmt clean
./run.sh status / storm / dry / smoke                          -> all exercised
./run.sh demo killed with SIGTERM                              -> both ports released
receiver stopped -> python3 scripts/demo.py storm              -> exit 1, "receiver unreachable"
no database     -> UI shows "No database at <path>" + NO TELEMETRY banner
stale > 30 s    -> UNKNOWN + "STALE — newest packet 2m 7s old"
390x844         -> no horizontal overflow, sticky strip, scene reframed via viewBox
1536x960        -> page and rail fit exactly, no clipping, no overlap between
                   verdict / gauges / banner / charts
impeccable detect                                             -> 27 findings -> 7 (all 11px label
                   warnings, intentional; no errors)
```

Measured (this laptop, release build, 240 packets in the window):

```
GET /api/data   17 KB, avg 3.43 ms, p50 3.39, p95 3.80, max 4.15 (n=30)
GET /           avg 1.15 ms
first paint     80 ms, DOMContentLoaded 31 ms, load 33 ms
render           ~2.6 ms per data refresh (2 canvases + SVG), refresh every 2 s
```

Before/after: the accept loop polled every 20 ms, giving avg 13.85 ms and max
15.37 ms per request. Poll is now 1 ms; the numbers above are the re-measure.

## Deliberate simplifications

- Polling every 2 s instead of SSE: two nodes at 0.1 Hz. Add SSE only if
  sub-second latency matters.
- One endpoint instead of `/status` + `/series`: one round trip.
- The evidence rule duplicates the four baseline thresholds from
  `analysis.rs` as literals, with a comment naming the source. When the backend
  exposes live inference, replace `decide()` and the `rule` block; the UI
  already renders `model.available`.

## Known gaps

- No live model score: backend step 4/5 has no inference endpoint. CRITICAL is
  therefore unreachable from real data (only independent displacement could
  raise it).
- Scenes/gauges show the last known packet when stale, with an explicit banner;
  they are not blanked.
- Screenshots were verified by DOM geometry and an automated design detector;
  the vision model was unavailable for a human-style look pass at the end.
  Files: `cache/scratch/final-1536.png`, `final-390.png`.
- Dashboard has no auth and binds loopback only. Do not expose the port.
- Device location can be set only from the machine running the dashboard; phone
  provisioning needs a LAN-exposed authenticated deployment or ESP captive
  portal posting the same `/api/site` contract.

#!/usr/bin/env bash
# SlopeGuard control script. Run "./run.sh help" for every command and for
# the troubleshooting of a dashboard that does not change.
set -euo pipefail
cd "$(dirname "$0")"

DB=${SG_DB:-data/raw/slopeguard.db}
TOKENS=${SG_TOKENS:-config/nodes.tokens}
ADDR=${SG_ADDR:-127.0.0.1:8080}
DADDR=${SG_DASH_ADDR:-127.0.0.1:8090}
RECV=rust/slopeguard/target/release/slopeguard
DASH=rust/dashboard/target/release/slopeguard-dashboard
RCARGO=rust/slopeguard/Cargo.toml
DCARGO=rust/dashboard/Cargo.toml

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

ensure_tokens() {
  [[ -f $TOKENS ]] || { cp config/nodes.tokens.example "$TOKENS"; say "created $TOKENS from the example (demo tokens)"; }
}

ensure_bin() {
  [[ -x $1 ]] || { say "building $(basename "$1")..."; cargo build --release --manifest-path "$2"; }
}

port_busy() {
  # 0.0.0.0 and :: bind attempts are what actually fail; grep the local table
  ss -ltn 2>/dev/null | awk '{print $4}' | grep -qx "$1"
}

wait_http() {
  for _ in $(seq 1 40); do
    curl -fsS -o /dev/null "$1" 2>/dev/null && return 0
    sleep 0.15
  done
  return 1
}

cmd_build() {
  cargo build --release --manifest-path "$RCARGO"
  cargo build --release --manifest-path "$DCARGO"
}

cmd_test() {
  say '== slopeguard tests'
  cargo test --manifest-path "$RCARGO"
  say '== dashboard tests'
  cargo test --manifest-path "$DCARGO"
  say '== clippy'
  cargo clippy --manifest-path "$RCARGO" --all-targets -- -D warnings
  cargo clippy --manifest-path "$DCARGO" --all-targets -- -D warnings
  say '== fmt'
  cargo fmt --manifest-path "$RCARGO" -- --check
  cargo fmt --manifest-path "$DCARGO" -- --check
  say 'all checks passed'
}

cmd_serve() {
  ensure_tokens
  ensure_bin "$RECV" "$RCARGO"
  port_busy "$ADDR" && die "port $ADDR is already in use"
  exec "$RECV" serve --addr "$ADDR" --db "$DB" --tokens "$TOKENS"
}

cmd_dash() {
  ensure_bin "$DASH" "$DCARGO"
  port_busy "$DADDR" && die "port $DADDR is already in use"
  exec "$DASH" --addr "$DADDR" --db "$DB" --web web \
    --receiver "http://$ADDR" --tokens "$TOKENS"
}

cmd_demo() {
  [[ ${1:-} == reset ]] && cmd_reset
  ensure_tokens
  ensure_bin "$RECV" "$RCARGO"
  ensure_bin "$DASH" "$DCARGO"
  port_busy "$ADDR" && die "port $ADDR is already in use (./run.sh status to see the running pair)"
  port_busy "$DADDR" && die "port $DADDR is already in use"

  "$RECV" serve --addr "$ADDR" --db "$DB" --tokens "$TOKENS" &
  RECV_PID=$!
  "$DASH" --addr "$DADDR" --db "$DB" --web web --receiver "http://$ADDR" --tokens "$TOKENS" &
  DASH_PID=$!
  trap 'kill ${RECV_PID:-} ${DASH_PID:-} 2>/dev/null || true' EXIT INT TERM

  wait_http "http://$DADDR/" || die "dashboard did not answer on http://$DADDR"
  say "dashboard  http://$DADDR"
  say "receiver   http://$ADDR   db=$DB"
  say "demo page: http://$DADDR/demo/   (or ./run.sh storm)"
  wait $DASH_PID
}

cmd_replay() {
  local scenario=${1:-storm}
  local body
  body=$(curl -sS -X POST "http://$DADDR/api/replay" \
    -H 'Content-Type: application/json' \
    -d "{\"scenario\":\"$scenario\",\"rate\":${SG_RATE:-30}}" 2>&1) \
    || die "dashboard not reachable at http://$DADDR (start it with ./run.sh demo)"
  say "$body"
  case $body in
    *replay_in_progress*) say 'a replay is still running; wait for it to finish' ;;
    *receiver_unreachable*) say 'receiver is down: ./run.sh serve (or ./run.sh demo)' ;;
    *'"accepted":0'*) say 'nothing new accepted: this fixture is already stored (stop, then ./run.sh demo reset)' ;;
  esac
}

cmd_status() {
  local token
  token=$(awk '!/^#/ && NF {print $2; exit}' "$TOKENS" 2>/dev/null || true)
  if port_busy "$DADDR"; then
    say "dashboard  up   http://$DADDR"
    summary=$(curl -sS "http://$DADDR/api/data?window_s=900" || true)
    say "  packets  $(printf '%s' "$summary" | grep -o '"packets":[0-9]*' | head -1 | cut -d: -f2)"
    say "  state    $(printf '%s' "$summary" | grep -o '"state":"[A-Z_]*"' | head -1 | cut -d'"' -f4)   quality $(printf '%s' "$summary" | grep -o '"quality":"[A-Z]*"' | head -1 | cut -d'"' -f4)"
    local n
    n=$(printf '%s' "$summary" | grep -o '"packets":[0-9]*' | head -1 | cut -d: -f2)
    [[ ${n:-0} -gt 0 ]] && say "  note     $n packets stored: replaying a stored fixture is a duplicate; ./run.sh demo reset starts clean"
  else
    say "dashboard  down http://$DADDR"
  fi
  if port_busy "$ADDR"; then
    say "receiver   up   http://$ADDR"
    [[ -n $token ]] && curl -sS -H "Authorization: Bearer $token" "http://$ADDR/health" || say 'no token in '"$TOKENS"
    say ''
  else
    say "receiver   down http://$ADDR"
  fi
  if [[ -f $DB ]]; then
    say "database   $DB  $(du -h "$DB" | cut -f1)"
  else
    say "database   $DB  (missing)"
  fi
}

cmd_reset() {
  if port_busy "$ADDR" || port_busy "$DADDR"; then
    die "a receiver or dashboard is running on $ADDR / $DADDR; stop it first (Ctrl-C), then ./run.sh demo reset"
  fi
  rm -f "$DB" "$DB-wal" "$DB-shm"
  say "reset $DB"
}

cmd_help() {
  cat <<'EOF'
SlopeGuard control script

  ./run.sh <command>

RUN
  demo [reset]   Build if needed, start the receiver on 127.0.0.1:8080 and the
                 dashboard on 127.0.0.1:8090, then wait in the foreground.
                 Ctrl-C stops both. "reset" deletes data/raw/slopeguard.db
                 first, which is the only way to replay a fixture again.
  serve          Receiver only (POST /telemetry, GET /health).
  dash           Dashboard only. Needs a receiver on :8080 to ingest.

CHECK
  test           Tests, clippy -D warnings and fmt --check for both crates.
  smoke          Backend HTTP contract test, NOT the dashboard. Starts a
                 throwaway receiver on 127.0.0.1:18099 with a temporary
                 database, replays 40 packets and asserts 202, 409, 401, 422,
                 404, /health and the stored row count. Touches no real data
                 and leaves nothing running.
  status         Which ports answer, packet count, rule state, receiver health.

FIXTURES
  storm          Push data/replays/synthetic_storm.jsonl:
                 20 simulated minutes of rain, 240 packets, ends in MOVEMENT_RISK.
  dry            Push data/replays/synthetic_dry_control.jsonl:
                 the calm control 25 minutes later, 40 packets, ends STABLE.
                 Both go through the real receiver with config/nodes.tokens.

WHY THE DASHBOARD MAY NOT CHANGE
  1. "0 accepted / N duplicate"
     The fixture is already in the database. Packets are keyed by
     (site, node, seq), so a replay is rejected as 409 by design. Stop the
     script and run:  ./run.sh demo reset   then replay again.
  2. It worked, then the state went UNKNOWN after ~30 s
     The site stopped sending. The dashboard must not claim "normal" when it
     cannot hear the slope, so it shows UNKNOWN plus a STALE banner. Replaying
     the same fixture will not fix that (409 duplicate); use "demo reset".
  3. The chart looks the same after a replay
     The fixture is byte-identical, so the curve is identical. Only the packet
     age and the strip move. That is expected.
  4. "receiver down" in the header, replay fails with "Connection refused"
     Only "dash" is running. Use "demo", or start "serve" in another shell.
  5. "port already in use"
     Another pair is running. ./run.sh status, then stop it. If it is not
     yours: SG_ADDR=127.0.0.1:18081 SG_DASH_ADDR=127.0.0.1:18091 ./run.sh demo

WHAT A GOOD DEMO LOOKS LIKE
  ./run.sh demo reset          # empty database, both processes up
  open http://127.0.0.1:8090   # UNKNOWN + "no telemetry" is correct here
  press Replay storm           # state climbs WATCH -> MOVEMENT_RISK in ~20 s
  press Dry control            # state returns to STABLE, rain stops
  drag the strip / press <- -> # inspect any moment of the recording
  ./run.sh storm               # same thing from a second shell

ENVIRONMENT
  SG_DB          database path        (default data/raw/slopeguard.db)
  SG_TOKENS      node token file      (default config/nodes.tokens)
  SG_ADDR        receiver address     (default 127.0.0.1:8080)
  SG_DASH_ADDR   dashboard address    (default 127.0.0.1:8090)
  SG_RATE        replay packets/sec   (default 30)

The UI itself: web/index.html, web/style.css, web/app.js. Plan and state:
UI_PLAN.md. Backend work log: PROGRESS.md.
EOF
}

case ${1:-help} in
  demo|all)  cmd_demo "${2:-}" ;;
  serve)     cmd_serve ;;
  dash|dashboard) cmd_dash ;;
  build)     cmd_build ;;
  test|check) cmd_test ;;
  smoke)     bash scripts/smoke_receiver.sh ;;
  storm)     cmd_replay storm ;;
  dry)       cmd_replay dry ;;
  status|ps) cmd_status ;;
  reset)     cmd_reset ;;
  help|-h|--help) cmd_help ;;
  *) die "unknown command '$1' — ./run.sh help" ;;
esac

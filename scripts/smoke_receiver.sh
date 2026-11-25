#!/usr/bin/env bash
# End-to-end smoke test of the live receiver: replay fixture -> HTTP POST ->
# SQLite -> /health. Uses curl only; no test framework. Run from the repo root.
set -euo pipefail

OUT=${1:-}
D=$(mktemp -d)
BIN=rust/slopeguard/target/release/slopeguard
PORT=18099
URL=http://127.0.0.1:$PORT
TM=move-token-0123456789
TR=ref-token-9876543210

cargo build --release --manifest-path rust/slopeguard/Cargo.toml >/dev/null
"$BIN" replay --out "$D/stable.jsonl" >/dev/null
printf 'IMU-MOVE-01 %s\nIMU-REF-01 %s\n' "$TM" "$TR" > "$D/tokens"

"$BIN" serve --addr 127.0.0.1:$PORT --db "$D/raw.db" --tokens "$D/tokens" > "$D/serve.log" 2>&1 &
SRV=$!
trap 'kill $SRV 2>/dev/null || true' EXIT
sleep 0.6
cat "$D/serve.log"

i=0
printf 'post statuses: '
while IFS= read -r line; do
  i=$((i + 1))
  if (( i % 2 == 1 )); then tok=$TM; else tok=$TR; fi
  curl -s -o /dev/null -w '%{http_code} ' -X POST \
    -H "Authorization: Bearer $tok" -H 'Content-Type: application/json' \
    --data-binary "$line" "$URL/telemetry"
done < "$D/stable.jsonl"
echo
echo "duplicate:    $(curl -s -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $TM" --data-binary "$(head -1 "$D/stable.jsonl")" "$URL/telemetry")"
echo "bad token:    $(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'Authorization: Bearer nope-000000000000' --data-binary "$(head -1 "$D/stable.jsonl")" "$URL/telemetry")"
echo "wrong node:   $(curl -s -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $TR" --data-binary "$(head -1 "$D/stable.jsonl")" "$URL/telemetry")"
echo "bad packet:   $(curl -s -X POST -H "Authorization: Bearer $TM" --data-binary '{"node_id":"IMU-MOVE-01","seq":1}' -w ' %{http_code}' "$URL/telemetry")"
echo "unknown path: $(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $TM" "$URL/x")"
echo "health:       $(curl -s -H "Authorization: Bearer $TM" "$URL/health")"

kill $SRV
trap - EXIT
echo "db raw_packet rows: $(sqlite3 "$D/raw.db" 'select count(*) from raw_packet' 2>/dev/null || echo 'sqlite3 not installed')"
echo "workdir: $D"

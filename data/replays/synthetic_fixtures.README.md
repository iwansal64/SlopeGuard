# synthetic fixtures — provenance

Both files here are synthetic. They are demo and pipeline fixtures, not
measurements. No accuracy, lead-time, or field-performance claim may be made
from them.

## synthetic_storm.jsonl

source      synthetic, hand-authored for the dashboard demo
content     240 packets: 120 ticks x (IMU-MOVE-01, IMU-REF-01), 10 s interval
site        SLOPE-001, base 2026-11-25T08:00:00+07:00, 20 minutes
scenario    rain ramps to 0.25 mm per 10 s, soil moisture 34 -> 73 %,
            movement pitch relative to the reference rises 0 -> ~5.9 deg
ends in     MOVEMENT_RISK (relative tilt above the 2 deg baseline, rate 0.47 deg/min)
seq         1..120 per node

## synthetic_dry_control.jsonl

source      synthetic, hand-authored as the calm control for the same event
content     40 packets: 20 ticks x (IMU-MOVE-01, IMU-REF-01), 10 s interval
site        SLOPE-001, base 2026-11-25T08:45:00+07:00 (25 min after the storm)
scenario    no rain, soil 70 -> 69 %, relative tilt ~0.05 deg
ends in     STABLE
seq         5001..5020 per node (distinct range so it can follow the storm)

Replaying either fixture twice is a duplicate: packets are keyed by
(site, node, seq). Use `./run.sh demo reset` for a clean database.

Regenerate: generators are deliberately not committed; the JSONL is committed
byte-for-byte so the demo is reproducible. Replace both with recorded rig data
before reporting any result.

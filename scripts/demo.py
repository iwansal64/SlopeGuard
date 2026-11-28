#!/usr/bin/env python3
"""Replay a recorded SlopeGuard scenario to a running receiver."""

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = {
    "storm": ROOT / "data/replays/synthetic_storm.jsonl",
    "dry": ROOT / "data/replays/synthetic_dry_control.jsonl",
    "stable": ROOT / "data/replays/stable_dry.jsonl",
}


def read_tokens(path):
    tokens = {}
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        parts = line.split()
        if len(parts) >= 2 and not parts[0].startswith("#"):
            tokens[parts[0]] = parts[1]
    return tokens


def post(receiver, token, body):
    request = urllib.request.Request(
        f"{receiver.rstrip('/')}/telemetry",
        data=body.encode(),
        method="POST",
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            return response.status
    except urllib.error.HTTPError as error:
        return error.code


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scenario", nargs="?", default="storm", choices=FIXTURES)
    parser.add_argument("--rate", type=int, default=int(os.environ.get("SG_RATE", "30")))
    parser.add_argument(
        "--receiver",
        default=os.environ.get(
            "SG_RECEIVER", os.environ.get("SG_ADDR", "127.0.0.1:8080")
        ),
    )
    parser.add_argument(
        "--tokens", default=os.environ.get("SG_TOKENS", "config/nodes.tokens")
    )
    args = parser.parse_args()
    if "://" not in args.receiver:
        args.receiver = "http://" + args.receiver

    fixture = FIXTURES[args.scenario]
    try:
        lines = [
            line for line in fixture.read_text(encoding="utf-8").splitlines() if line.strip()
        ]
        tokens = read_tokens(args.tokens)
    except OSError as error:
        parser.error(f"{error} (start ./run.sh demo once to create the token file)")

    interval = 1.0 / max(1, min(args.rate, 60))
    accepted = duplicate = rejected = 0
    print(
        f"{args.scenario}: {len(lines)} packets -> {args.receiver} at "
        f"{max(1, min(args.rate, 60))}/s"
    )
    for line in lines:
        started = time.monotonic()
        packet = json.loads(line)
        node = packet["node_id"]
        token = tokens.get(node)
        if token is None:
            print(f"no token for {node} in {args.tokens}", file=sys.stderr)
            return 2
        try:
            status = post(args.receiver, token, line)
        except OSError as error:
            print(f"receiver unreachable at {args.receiver}: {error}", file=sys.stderr)
            return 1
        if status == 202:
            accepted += 1
        elif status == 409:
            duplicate += 1
        else:
            rejected += 1
            print(f"{node} seq {packet['seq']}: HTTP {status}", file=sys.stderr)
            if rejected >= 10:
                print("too many rejects, stopping", file=sys.stderr)
                break
        delay = interval - (time.monotonic() - started)
        if delay > 0:
            time.sleep(delay)
    print(f"accepted {accepted}, duplicate {duplicate}, rejected {rejected}")
    if accepted == 0 and duplicate > 0:
        print("already in the database; ./run.sh demo reset then replay", file=sys.stderr)
    return 0 if accepted or duplicate else 1


if __name__ == "__main__":
    sys.exit(main())

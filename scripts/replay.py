#!/usr/bin/env python3
"""Replays a recorded hook log into a running Agent Room app.

Each line of the log is {"ts_ms": <unix ms>, "hook": <hook JSON>}, as written by
`room-hook --log`. Every hook is piped into `room-hook` (forward mode) with the
original spacing divided by --speed. Permission requests run in the background,
because room-hook waits for an answer.

    scripts/replay.py ~/.agent-room/spike/events.jsonl --speed 4
"""

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_HOOK = ROOT / "target" / "debug" / "room-hook"
MAX_GAP_S = 3.0  # don't wait through long idle stretches


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("log", type=Path)
    parser.add_argument("--speed", type=float, default=1.0, help="replay speed factor (default 1)")
    parser.add_argument("--hook", type=Path, default=DEFAULT_HOOK, help="room-hook binary")
    parser.add_argument("--session", help="only replay this session id (prefix)")
    args = parser.parse_args()

    if not args.hook.exists():
        print(f"{args.hook} not found; run `cargo build -p room-hook`", file=sys.stderr)
        return 1

    waiting = []
    previous_ts = None
    for line in args.log.read_text().splitlines():
        if not line.strip():
            continue
        record = json.loads(line)
        hook = record["hook"]
        if args.session and not hook.get("session_id", "").startswith(args.session):
            continue
        if previous_ts is not None:
            time.sleep(min((record["ts_ms"] - previous_ts) / 1000 / args.speed, MAX_GAP_S))
        previous_ts = record["ts_ms"]

        payload = json.dumps(hook).encode()
        name = hook.get("hook_event_name", "?")
        print(f"{name:20} {hook.get('tool_name', '')}")
        if name == "PermissionRequest":
            proc = subprocess.Popen([str(args.hook)], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
            proc.stdin.write(payload)
            proc.stdin.close()
            waiting.append(proc)
        else:
            subprocess.run([str(args.hook)], input=payload, check=False)

    for proc in waiting:
        answer = proc.stdout.read().decode().strip()
        proc.wait()
        print(f"permission answer: {answer or '(none: terminal prompt)'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

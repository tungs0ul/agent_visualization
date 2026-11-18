#!/usr/bin/env bash
# Measures Agent Room's CPU and memory in the states of the V2.md power budget.
#
#   scripts/measure.sh
#   MEASURE_SECONDS=10 scripts/measure.sh    # shorter samples
#   MEASURE_SPEED=4 scripts/measure.sh       # replay 4x faster
#
# Builds the release app, starts it with a throwaway AGENT_ROOM_DIR, hands focus to
# Finder (the app usually sits unfocused beside the terminal), then samples `top`:
#   idle    no Claude activity (3D: robots' idle clip at about 4 fps)
#   dozing  no activity for over 30 s (3D stops redrawing)
#   active  while the spike log replays at real speed (MEASURE_SPEED=4: faster,
#           so robots walk nearly all the time: the worst case)
# Prints mean CPU (% of one core) and peak memory per state, and kills everything it
# started on exit.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOG="${MEASURE_LOG:-$HOME/.agent-room/spike/events.jsonl}"
SECONDS_PER_STATE="${MEASURE_SECONDS:-20}"
DIR="$(mktemp -d "${TMPDIR:-/tmp}/ar-measure.XXXX")"
APP_PID=""
REPLAY_PID=""

cleanup() {
    [[ -n "$REPLAY_PID" ]] && pkill -P "$REPLAY_PID" 2>/dev/null || true
    [[ -n "$REPLAY_PID" ]] && kill "$REPLAY_PID" 2>/dev/null || true
    [[ -n "$APP_PID" ]] && kill "$APP_PID" 2>/dev/null || true
    wait 2>/dev/null || true
    rm -rf "$DIR"
}
trap cleanup EXIT

cargo build --release -q --manifest-path "$ROOT/Cargo.toml" -p room-app -p room-hook

AGENT_ROOM_DIR="$DIR" "$ROOT/target/release/agent-room" >"$DIR/app.log" 2>&1 &
APP_PID=$!
sleep 6
osascript -e 'tell application "Finder" to activate' >/dev/null 2>&1 || true
sleep 4

# Prints "<mean cpu> <peak mem MB>" over $SECONDS_PER_STATE one-second samples.
sample() {
    top -l "$((SECONDS_PER_STATE + 1))" -s 1 -pid "$APP_PID" -stats cpu,mem \
        | awk '
            /^[0-9.]+ +[0-9.]+[KMG]/ {
                n++; if (n == 1) next   # the first sample has no CPU delta
                cpu += $1
                m = $2; unit = substr(m, length(m)); v = m + 0
                if (unit == "K") v /= 1024; else if (unit == "G") v *= 1024
                if (v > mem) mem = v
            }
            END { if (n > 1) printf "%.1f %.0f\n", cpu / (n - 1), mem; else print "? ?" }'
}

read -r idle_cpu idle_mem < <(sample)
sleep 15
read -r doze_cpu doze_mem < <(sample)

active_cpu="-"; active_mem="-"
if [[ -f "$LOG" ]]; then
    AGENT_ROOM_DIR="$DIR" python3 "$ROOT/scripts/replay.py" "$LOG" --speed "${MEASURE_SPEED:-1}" \
        --hook "$ROOT/target/release/room-hook" >/dev/null 2>&1 &
    REPLAY_PID=$!
    sleep 3
    read -r active_cpu active_mem < <(sample)
else
    echo "no replay log at $LOG; skipping the active state" >&2
fi

printf '%-8s %8s %8s\n' state "cpu %" "mem MB"
printf '%-8s %8s %8s\n' idle "$idle_cpu" "$idle_mem"
printf '%-8s %8s %8s\n' dozing "$doze_cpu" "$doze_mem"
printf '%-8s %8s %8s\n' active "$active_cpu" "$active_mem"

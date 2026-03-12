#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TLOG="$WORKSPACE_ROOT/state/kernel_logs/kernel.tlog"
TLOG_IDX="$WORKSPACE_ROOT/state/kernel_logs/kernel.tlog.idx"
REPORTS_OUT="$WORKSPACE_ROOT/state/graph"
REPORTS_BIN="$WORKSPACE_ROOT/target/debug/reports_from_tlog"
DEBOUNCE=1  # seconds of silence before triggering

echo "canon: watching $TLOG_IDX for build completions..."

# Ensure the logs dir exists so inotifywait doesn't fail on first run
mkdir -p "$(dirname "$TLOG_IDX")"
touch "$TLOG_IDX"

last_seen=0
reports_pid=""

while true; do
    # Block until any close_write on the idx file
    inotifywait -q -e close_write "$TLOG_IDX" 2>/dev/null || sleep 0.2

    last_seen=$(date +%s%N)

    # Drain: keep resetting until silence for $DEBOUNCE seconds
    while true; do
        inotifywait -q -e close_write -t "$DEBOUNCE" "$TLOG_IDX" 2>/dev/null && {
            last_seen=$(date +%s%N)
            continue
        }
        break
    done

    if [ ! -f "$TLOG" ]; then
        echo "canon: tlog not found, skipping" >&2
        continue
    fi

    if [ ! -f "$REPORTS_BIN" ]; then
        echo "canon: reports_from_tlog not built, skipping" >&2
        continue
    fi

    echo "canon: build quiet — generating reports..."

    # Kill any still-running previous invocation before spawning a new one
    if [ -n "$reports_pid" ] && kill -0 "$reports_pid" 2>/dev/null; then
        echo "canon: killing previous reports run (pid $reports_pid)"
        kill "$reports_pid" 2>/dev/null || true
        wait "$reports_pid" 2>/dev/null || true
    fi

    "$REPORTS_BIN" \
        --tlog "$TLOG" \
        --out  "$REPORTS_OUT" \
        </dev/null 2>&1 | sed 's/^/canon: /' &
    reports_pid=$!
done

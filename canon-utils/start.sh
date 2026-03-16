#!/usr/bin/env bash
# start.sh — run canon-kernel directly
#
# NOTE: cargo run --bin canon-supervisor (or ./target/debug/canon-supervisor)
#       is intended to run in a SEPARATE terminal from the repo root
#       (/workspace/ai_sandbox/canon). It watches for file changes and
#       automatically rebuilds + restarts canon-kernel. Use that for
#       active development. This script is for running the kernel standalone.

set -e

REPO_ROOT="/workspace/ai_sandbox/canon"
UTILS_ROOT="$REPO_ROOT/canon-utils"
TLOG="$REPO_ROOT/state/event_log/event.tlog.d"

cd "$UTILS_ROOT"

echo "[start] creating state directories..."
mkdir -p "$REPO_ROOT/state/event_log/event.tlog.d"
mkdir -p "$REPO_ROOT/state/reports_out"
mkdir -p "$REPO_ROOT/state/projections"

# Remove stale lock if the previous process is dead
LOCK="$REPO_ROOT/state/event_runtime.lock"
if [ -f "$LOCK" ]; then
    PID=$(grep -oP '(?<=pid=)\d+' "$LOCK" 2>/dev/null || true)
    if [ -n "$PID" ] && ! kill -0 "$PID" 2>/dev/null; then
        echo "[start] removing stale lock (pid=$PID no longer running)"
        rm -f "$LOCK"
    fi
fi

echo "[start] starting canon-kernel (tlog: $TLOG)"
echo "[start] LLM execution enabled — browser extension must be connected on port 9100"
echo ""

CANON_EVENT_EXECUTION=1 \
CANON_REPORTS_TLOG="$TLOG" \
CANON_REPORTS_OUT="$REPO_ROOT/state/reports_out" \
CANON_EVENT_RUNTIME_LOG="$REPO_ROOT/state/event_runtime.log" \
CANON_REPORTS_VERIFY_DETERMINISM=1 \
CANON_REPORTS_VERIFY_LAYOUT=1 \
cargo run -p canon-kernel --bin canon-kernel -- --tlog "$TLOG"

# ---------------------------------------------------------------------------
# Useful one-liners (run in a separate terminal):
#
# Emit a test LLM capability:
#   cargo run -p canon-event-store --bin emit_capability_event -- \
#     --tlog /workspace/ai_sandbox/canon/state/event_log/event.tlog.d \
#     --name llm.call \
#     --args '{"prompt":"hello","role":"exec","raw":true}'
#
# Read tlog events:
#   cargo run -p canon-event-store --bin read_tlog -- \
#     /workspace/ai_sandbox/canon/state/event_log/event.tlog.d
# ---------------------------------------------------------------------------

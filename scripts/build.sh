#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TLOG="$WORKSPACE_ROOT/kernel/logs/graph.tlog"
REPORTS_OUT="$WORKSPACE_ROOT/kernel/graph"
REPORTS_BIN="$WORKSPACE_ROOT/target/debug/reports_from_tlog"

# ── build ────────────────────────────────────────────────────────────────────
cargo build "$@"
BUILD_EXIT=$?

if [ $BUILD_EXIT -ne 0 ]; then
    exit $BUILD_EXIT
fi

# ── report generation ────────────────────────────────────────────────────────
if [ ! -f "$TLOG" ]; then
    echo "canon: tlog not found at $TLOG, skipping reports" >&2
    exit 0
fi

if [ ! -f "$REPORTS_BIN" ]; then
    echo "canon: reports_from_tlog not built yet, building..." >&2
    cargo build --bin reports_from_tlog -p canon_reports
fi

"$REPORTS_BIN" \
    --tlog "$TLOG" \
    --out  "$REPORTS_OUT" \
    </dev/null >/dev/null 2>&1 &

echo "canon: reports generation started (pid $!)"

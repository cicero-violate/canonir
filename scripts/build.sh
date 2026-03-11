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

echo "canon: build complete — reports watcher will trigger automatically"

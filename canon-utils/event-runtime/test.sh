#!/usr/bin/env bash
set -euo pipefail

TLOG_PATH="${CANON_TLOG_PATH:-/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog.d}"
LOCK_PATH="${CANON_EVENT_RUNTIME_LOCK:-/workspace/ai_sandbox/canon/state/event_runtime.lock}"

echo "llm_smoke_test: using live runtime tlog at ${TLOG_PATH}"

CANON_EVENT_RUNTIME_LOCK="${LOCK_PATH}" \
CANON_TLOG_PATH="${TLOG_PATH}" \
cargo run -p canon-event-runtime --bin llm_smoke_test

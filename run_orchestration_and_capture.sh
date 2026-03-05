#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
LOG_DIR="$ROOT/agent_logs"
mkdir -p "$LOG_DIR"

LOG_FILE="$LOG_DIR/orchestration_pipeline_run.log"

cd "$ROOT"

echo "=== ORCHESTRATION PIPELINE RUN ===" | tee "$LOG_FILE"
echo "Timestamp: $(date)" | tee -a "$LOG_FILE"
echo "Command: cargo run --bin orchestration -- --all" | tee -a "$LOG_FILE"
echo "-------------------------------------------" | tee -a "$LOG_FILE"

# Run orchestration and capture both stdout and stderr
(
  cargo run --bin orchestration -- --all
) 2>&1 | tee -a "$LOG_FILE"

EXIT_CODE=${PIPESTATUS[0]}

echo "-------------------------------------------" | tee -a "$LOG_FILE"
echo "Exit code: $EXIT_CODE" | tee -a "$LOG_FILE"
echo "Log written to: $LOG_FILE" | tee -a "$LOG_FILE"

exit $EXIT_CODE

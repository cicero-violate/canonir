#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
LOG_DIR="$ROOT/agent_logs"
mkdir -p "$LOG_DIR"

OUT="$LOG_DIR/orchestration_run.log"

cd "$ROOT"

{
  echo "=== ORCHESTRATION RUN START ==="
  date
  echo "Running: cargo run --bin orchestration -- --all"

  cargo run --bin orchestration -- --all

  echo "=== ORCHESTRATION RUN END ==="
  date
} 2>&1 | tee "$OUT"

echo "Log written to $OUT"

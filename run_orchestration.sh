#!/usr/bin/env bash
# run_orchestration.sh — end-to-end emit → capture → re-emit comparison
set -euo pipefail

ROOT="/workspace/ai_sandbox/canon"
BASE="$ROOT/test_projects/test_rust_projects"

INPUT_IR="$BASE/model_ir.json"
EMIT_OUT="$BASE/emit/test_1"

CAPTURE_OUT="$BASE/capture/test_1"
CAPTURE_IR="$CAPTURE_OUT/model_ir_captured.json"

echo "=== build workspace ==="
cargo build --workspace

echo "=== emit from $INPUT_IR -> $EMIT_OUT ==="
rm -rf "$EMIT_OUT"
cargo run -p orchestration -- "$INPUT_IR" "$EMIT_OUT"

echo "=== capture emitted project -> $CAPTURE_IR ==="
rm -rf "$CAPTURE_OUT"
./run_capture.sh "$EMIT_OUT" "$CAPTURE_IR"

echo "=== re-emit from captured IR -> $CAPTURE_OUT ==="
cargo run -p orchestration -- "$CAPTURE_IR" "$CAPTURE_OUT"

echo "=== build captured re-emit ==="
(
  cd "$CAPTURE_OUT"
  cargo build
)

echo "=== JSON header diff (input vs captured) ==="
python "$BASE/compare.py" "$INPUT_IR" "$CAPTURE_IR"

echo "=== source diff (capture vs emit) ==="
python "$BASE/diff_src_dirs.py" \
  "$CAPTURE_OUT/src" \
  "$EMIT_OUT/src"

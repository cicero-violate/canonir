#!/usr/bin/env bash
# run_orchestration.sh — end-to-end emit → capture → re-emit comparison
# Usage:
#   ./run_orchestration.sh [<input_ir.json> [<emit_dir> [<capture_dir> [<template_dir>]]]]
# Defaults:
#   input_ir    = test_projects/test_rust_projects/model_ir.json
#   emit_dir    = test_projects/test_rust_projects/emit/test_1
#   capture_dir = test_projects/test_rust_projects/capture/test_1
set -euo pipefail

ROOT="/workspace/ai_sandbox/canon"
BASE="$ROOT/test_projects/test_rust_projects"

INPUT_IR="${1:-$BASE/model_ir.json}"
EMIT_OUT="${2:-$BASE/emit/test_1}"
CAPTURE_OUT="${3:-$BASE/capture/test_1}"
TEMPLATE_DIR="${4:-}"
CAPTURE_IR="$CAPTURE_OUT/model_ir_captured.json"

echo "=== build workspace ==="
cargo build --workspace

# If the input IR is missing but a template_dir is provided, capture it first.
if [ ! -f "$INPUT_IR" ] && [ -n "$TEMPLATE_DIR" ]; then
  echo "=== input IR missing; capturing from template $TEMPLATE_DIR -> $INPUT_IR ==="
  mkdir -p "$(dirname "$INPUT_IR")"
  ./run_capture.sh "$TEMPLATE_DIR" "$INPUT_IR"
fi

echo "=== emit from $INPUT_IR -> $EMIT_OUT ==="
rm -rf "$EMIT_OUT"
cargo run -p orchestration -- "$INPUT_IR" "$EMIT_OUT"

if [ -n "$TEMPLATE_DIR" ]; then
  echo "=== overlay template from $TEMPLATE_DIR into $EMIT_OUT (deps/src) ==="
  if [ -f "$TEMPLATE_DIR/Cargo.toml" ]; then
    cp "$TEMPLATE_DIR/Cargo.toml" "$EMIT_OUT/Cargo.toml"
  fi
  if [ -d "$TEMPLATE_DIR/src" ]; then
    mkdir -p "$EMIT_OUT/src"
    rsync -a --delete "$TEMPLATE_DIR/src/" "$EMIT_OUT/src/"
  fi
fi

echo "=== capture emitted project -> $CAPTURE_IR ==="
# Only wipe the capture output if it differs from the template; preserve template sources.
if [ -z "$TEMPLATE_DIR" ] || [ "$CAPTURE_OUT" != "$TEMPLATE_DIR" ]; then
  rm -rf "$CAPTURE_OUT"
fi
mkdir -p "$CAPTURE_OUT"

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

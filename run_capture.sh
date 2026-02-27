#!/usr/bin/env bash
# run_capture.sh — capture a Rust project into CanonIR JSON
#
# Usage: ./run_capture.sh <path/to/project> <output.json>
#
# Example:
  # ./run_capture.sh test_projects/test_captured_project \
  #     test_projects/test_captured_project/canon_ir_captured.json

set -euo pipefail

PROJECT_DIR="${1:?usage: run_capture.sh <project_dir> <output_json>}"
OUTPUT_JSON="${2:?usage: run_capture.sh <project_dir> <output_json>}"

# Resolve absolute paths.
PROJECT_DIR="$(realpath "$PROJECT_DIR")"
OUTPUT_JSON="$(realpath -m "$OUTPUT_JSON")"

# Build the capture wrapper first.
echo "Building rustc_capture..."
cargo build -p rustc_capture 2>&1

WRAPPER="$(cargo metadata --no-deps --format-version 1 \
    | python3 -c "import sys,json; d=json.load(sys.stdin); \
      print([w['target_directory'] for w in [d]][0])")/debug/rustc_capture"

# Fallback: find it directly.
if [ ! -f "$WRAPPER" ]; then
    WRAPPER="$(dirname "$(cargo locate-project --workspace --message-format plain)")/target/debug/rustc_capture"
fi

REAL_RUSTC="$(rustup which rustc)"

echo "Capturing $PROJECT_DIR -> $OUTPUT_JSON"
echo "  wrapper:    $WRAPPER"
echo "  real rustc: $REAL_RUSTC"

mkdir -p "$(dirname "$OUTPUT_JSON")"

# Force recompile so the wrapper always fires.
rm -rf "$PROJECT_DIR/target_capture"
mkdir -p "$PROJECT_DIR/target_capture"

# Remove stale output so the capture binary always writes fresh IR.
rm -f "$OUTPUT_JSON"

CANON_CAPTURE_OUT="$OUTPUT_JSON" \
RUSTC_WRAPPER="$WRAPPER" \
CARGO_NET_OFFLINE="${CARGO_NET_OFFLINE:-true}" \
    cargo build \
        --manifest-path "$PROJECT_DIR/Cargo.toml" \
        --target-dir "$PROJECT_DIR/target_capture" \
        2>&1

if [ -f "$OUTPUT_JSON" ]; then
    echo "Done. IR written to $OUTPUT_JSON"
    echo "Nodes: $(python3 -c "import json; d=json.load(open('$OUTPUT_JSON')); print(len(d.get('nodes', [])))")"
else
    echo "ERROR: $OUTPUT_JSON was not produced."
    exit 1
fi

#!/usr/bin/env bash
set -euo pipefail

WORKSPACE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
echo "Workspace root: $WORKSPACE_ROOT"

echo "Building rustc_capture_wrapper..."
cargo build -p rustc_capture_wrapper

WRAPPER="$WORKSPACE_ROOT/target/debug/rustc_capture_wrapper"
echo "Wrapper: $WRAPPER"

echo "Cleaning orchestration crate..."
cargo clean -p orchestration

echo "Building orchestration WITHOUT wrapper..."
cargo build -p orchestration

echo "Running orchestration WITH wrapper for downstream builds..."

TOOLCHAIN_LIB="$(rustc --print sysroot)/lib"

LD_LIBRARY_PATH="$TOOLCHAIN_LIB:$LD_LIBRARY_PATH" \
RUSTC_WRAPPER="$WRAPPER" \
RUSTC_BOOTSTRAP=1 \
"$WORKSPACE_ROOT/target/debug/orchestration" \
"$WORKSPACE_ROOT/orchestration"

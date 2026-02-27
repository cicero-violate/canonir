#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

"$SCRIPT_DIR/run_capture.sh" /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap/canon_capture.json
rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap/
cargo run -p orchestration -- \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap/canon_capture.json \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap

cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap && CARGO_NET_OFFLINE="${CARGO_NET_OFFLINE:-true}" cargo build

python /workspace/ai_sandbox/canon/test_projects/test_rust_projects/diff_src_dirs.py \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap/src \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap/src


"$SCRIPT_DIR/run_capture.sh" /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1 /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1/canon_capture.json
rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/test_1/
cargo run -p orchestration -- \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1/canon_capture.json \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/test_1

cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/test_1 && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1 && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/test_1 && CARGO_NET_OFFLINE="${CARGO_NET_OFFLINE:-true}" cargo build

python /workspace/ai_sandbox/canon/test_projects/test_rust_projects/diff_src_dirs.py \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1/src \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/test_1/src

"$SCRIPT_DIR/run_capture.sh" /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/semantic-lint /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/semantic-lint/canon_capture.json
rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/semantic-lint/
cargo run -p orchestration -- \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/semantic-lint/canon_capture.json \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/semantic-lint

cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/semantic-lint && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/semantic-lint && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/semantic-lint && CARGO_NET_OFFLINE="${CARGO_NET_OFFLINE:-true}" cargo build

python /workspace/ai_sandbox/canon/test_projects/test_rust_projects/diff_src_dirs.py \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/semantic-lint/src \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/semantic-lint/src

"$SCRIPT_DIR/run_capture.sh" /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/conversation /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/conversation/canon_capture.json
rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/conversation/
cargo run -p orchestration -- \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/conversation/canon_capture.json \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/conversation

cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/conversation && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/conversation && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/conversation && CARGO_NET_OFFLINE="${CARGO_NET_OFFLINE:-true}" cargo build

python /workspace/ai_sandbox/canon/test_projects/test_rust_projects/diff_src_dirs.py \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/conversation/src \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/conversation/src

"$SCRIPT_DIR/run_capture.sh" /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/canon /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/canon/canon_capture.json
rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/canon/
cargo run -p orchestration -- \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/canon/canon_capture.json \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/canon

cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/canon && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/canon && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/canon && CARGO_NET_OFFLINE="${CARGO_NET_OFFLINE:-true}" cargo build

python /workspace/ai_sandbox/canon/test_projects/test_rust_projects/diff_src_dirs.py \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/canon/src \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/canon/src

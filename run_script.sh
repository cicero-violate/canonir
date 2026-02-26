#!/usr/bin/env bash
set -euo pipefail

./run_capture.sh /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap/canon_capture.json
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


./run_capture.sh /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1 /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1/canon_capture.json
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

./run_capture.sh /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/semantic-lint /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/semantic-lint/canon_capture.json
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

./run_capture.sh /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/mir_tracer /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/mir_tracer/canon_capture.json
rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/mir_tracer/
cargo run -p orchestration -- \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/mir_tracer/canon_capture.json \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/mir_tracer

cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/mir_tracer && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/mir_tracer && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/mir_tracer && CARGO_NET_OFFLINE="${CARGO_NET_OFFLINE:-true}" cargo build

python /workspace/ai_sandbox/canon/test_projects/test_rust_projects/diff_src_dirs.py \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/mir_tracer/src \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/mir_tracer/src

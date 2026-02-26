./run_capture.sh /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap/capture.json
rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap/
cargo run -p orchestration -- \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap/capture.json \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap \
--canon

cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap && cargo build

python /workspace/ai_sandbox/canon/test_projects/test_rust_projects/diff_src_dirs.py \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap/src \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap/src


./run_capture.sh /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1 /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1/capture.json
rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/test_1/
cargo run -p orchestration -- \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1/capture.json \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/test_1 \
--canon

cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/test_1 && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1 && cargo fmt
cd /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/test_1 && cargo build

python /workspace/ai_sandbox/canon/test_projects/test_rust_projects/diff_src_dirs.py \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/test_1/src \
/workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/test_1/src

cargo build --workspace


rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_project/model_ir_captured.json

rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_project/test_emit/
cargo run -p orchestration -- \
  test_projects/test_rust_project/model_ir.json \
  test_projects/test_rust_project/test_emit

rm -rf /workspace/ai_sandbox/canon/test_projects/test_rust_project/test_capture/
./run_capture.sh test_projects/test_rust_project/test_emit/ \
    test_projects/test_rust_project/test_capture/model_ir_captured.json

cargo run -p orchestration -- \
  test_projects/test_rust_project/test_capture/model_ir_captured.json \
  test_projects/test_rust_project/test_capture

cd /workspace/ai_sandbox/canon/test_projects/test_rust_project/test_capture
cargo build

cp /workspace/ai_sandbox/canon/test_projects/test_rust_project/test_capture/model_ir_captured.json /workspace/ai_sandbox/canon/test_projects/test_rust_project/

echo "head -50 model_ir.json\n"
head -50 /workspace/ai_sandbox/canon/test_projects/test_rust_project/model_ir.json
echo "head -50 model_ir_captured.json\n"
head -50 /workspace/ai_sandbox/canon/test_projects/test_rust_project/model_ir_captured.json

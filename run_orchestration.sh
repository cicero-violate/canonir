rm -rf test_projects/test_rust_project/test_emit/
cargo run -p orchestration -- \
  test_projects/test_rust_project/model_ir.json \
  test_projects/test_rust_project/test_emit

cd test_projects/test_rust_project/test_emit && cargo build
cargo run


rm -rf test_projects/test_rust_project/test_capture/
./run_capture.sh test_projects/test_rust_project/test_emit/ \
    test_projects/test_rust_project/test_capture/model_ir_captured.json

cargo run -p orchestration -- \
  test_projects/test_captured_project/model_ir_captured.json \
  test_projects/test_captured_project

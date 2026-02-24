rm -rf test_projects/test_rust_project/
cargo run -p orchestration -- \
  test_projects/test_rust_project/model_ir.json \
  test_projects/test_rust_project/test_emit

cd test_projects/test_rust_project/test_emit && cargo build
cargo run

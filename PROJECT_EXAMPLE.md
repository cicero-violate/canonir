# PROJECT_EXAMPLE.md
## Verified Execution

```bash
# Build workspace
cargo build --workspace

# Capture CanonIR for test_1
./run_capture.sh \
  test_projects/test_rust_projects/capture/test_1 \
  test_projects/test_rust_projects/capture/test_1/canon_capture.json

# Orchestrate Canon pipeline for test_1
rm -rf test_projects/test_rust_projects/emit/test_1
cargo run -p orchestration -- \
  test_projects/test_rust_projects/capture/test_1/canon_capture.json \
  test_projects/test_rust_projects/emit/test_1
cd test_projects/test_rust_projects/emit/test_1 && CARGO_NET_OFFLINE=true cargo build

# Capture CanonIR for repomap
./run_capture.sh \
  test_projects/test_rust_projects/capture/repomap \
  test_projects/test_rust_projects/capture/repomap/canon_capture.json

# Orchestrate Canon pipeline for repomap
rm -rf test_projects/test_rust_projects/emit/repomap
cargo run -p orchestration -- \
  test_projects/test_rust_projects/capture/repomap/canon_capture.json \
  test_projects/test_rust_projects/emit/repomap
cd test_projects/test_rust_projects/emit/repomap && CARGO_NET_OFFLINE=true cargo build
```

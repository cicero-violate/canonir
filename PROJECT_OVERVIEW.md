# PROJECT_OVERVIEW.md

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

## Core Crates

```text
canon/
  ir.rs            — CanonIR (arena + intern tables + 8 CSR graphs)
  node.rs          — CanonNodeKind, TypeKind, CfgOp, flags
  edge.rs          — Canon-owned EdgeKind
  id.rs            — Canon-owned NodeId
  csr_graph.rs     — Canon-owned CSR graph

canon-capture/
  lib.rs           — rustc frontend capture entrypoint
  index.rs         — deterministic DefId -> NodeId index
  project/         — item/body/relation projection into partial payload
  canon_assemble.rs— deterministic Partial -> CanonIR assembly

canon-analyzer/
  lib.rs           — canon_analyze(ir)
  derive.rs        — graph derivation
  graph/           — 8 graph builders
  solver/          — Canon solver chain

canon-projection/
  layout/          — file/item planning
  emit/            — Canon source/Cargo emission

canon-mutation/
  lib.rs           — Canon mutation ops + diff + verify

orchestration/
  main.rs          — Canon-only pipeline entrypoint
```

## Pipeline Invariant

`Capture -> CanonIR -> Graph -> Solve -> Emit`

# PROJECT_OVERVIEW.md

## Verified Execution

```bash
# Model pipeline (legacy)
rm -rf test_projects/test_rust_project/test_emit
cargo run -p orchestration -- \
  test_projects/test_rust_project/model_ir.json \
  test_projects/test_rust_project/test_emit \
  --model
cd test_projects/test_rust_project/test_emit && cargo build

# Canon pipeline for test_1 (default)
rm -rf test_projects/test_rust_projects/emit/test_1
cargo run -p orchestration -- \
  test_projects/test_rust_projects/capture/test_1/capture.json \
  test_projects/test_rust_projects/emit/test_1
cd test_projects/test_rust_projects/emit/test_1 && cargo build

# Canon pipeline for repomap (default)
rm -rf test_projects/test_rust_projects/emit/repomap
cargo run -p orchestration -- \
  test_projects/test_rust_projects/capture/repomap/capture.json \
  test_projects/test_rust_projects/emit/repomap
cd test_projects/test_rust_projects/emit/repomap && cargo build
```

### Core files to know

```text
model/src/ir/
  mod.rs          — re-exports all IR modules
  node.rs         — NodeId, NodeKind, Body, BasicBlock, Stmt, Terminator,
                    Field, Param, GenericParam, EnumVariant, Visibility
  edge.rs         — EdgeKind: Contains, Calls, Resolves, Renames, TypeOf,
                    TypeUnifies, CfgEdge, CfgBranch, Outlives, ConstDep, Expands
  csr_graph.rs    — CsrGraph<ND,ED>: from_edges(), neighbours(), Default
  model_ir.rs     — ModelIR: nodes, emit_order, edge_hints,
                    8 CsrGraphs (name, type, call, module, cfg, region, value, macro),
                    cargo_dependencies
  model_diff.rs   — diff_semantic covers all 8 graphs + edge_hints + emit_order

canon/src/
  ir.rs           — CanonIR, intern tables, 8 CSR graphs, emit_order
  node.rs         — CanonNodeKind, TypeKind, CfgOp, flags bitfield
  seal.rs         — seal(ModelIR) -> CanonIR
                    now enriches composite payload (fields/variants/generics/trait methods/derives)

analyzer/src/
  lib.rs          — analyze(ir) = derive() + solve() on ModelIR
  derive.rs       — routes edge_hints into 8 graph builders
  graph/          — name/type/call/module/cfg/region/value/macro builders
  solver/         — full ModelIR solver chain

canon-analyzer/src/
  lib.rs          — canon_analyze(ir) = derive() + solve() on CanonIR
  derive.rs       — runs Canon graph builders and unions derived edges with sealed edges
  graph/          — Canon-native builders for all 8 graphs
  solver/         — Canon solver chain (invariant/module/type/call/cfg/...)
                    Canon mode currently skips aggressive name/use mutation steps

projection/src/
  layout/         — Model layout passes
  emit/           — Model emitter split by concern (file/items/functions/impls/types/body/fmt/cargo)

canon-projection/src/
  layout/         — Canon layout plan builder + dependency inference
  emit/           — Canon emitter split like projection:
                    file/items/functions/impls/types/body/fmt/cargo/macros/helpers
                    Type rendering is structural via TypeKind

orchestration/src/main.rs
  args: <ir.json> <output_dir> [--mutate <mutation.json>] [--model]
  mode default: Canon pipeline
    load CanonIR (or ModelIR shim->seal) -> canon_analyze -> canon_projection -> canon_ir_solved.json
  mode --model: legacy Model pipeline only
    load -> analyze -> (optional mutate/verify/diff) -> projection -> emit -> model_ir_solved.json
```

## Design Properties

- IR remains canonical state.
- 8 CSR graphs are the shared constraint representation.
- Solvers enforce legality before emit.
- Emit layers are split into pure renderers (no traversal/sorting/mutation in emit).
- Orchestration mode selection is explicit:
  - default = Canon pipeline
  - `--model` = legacy Model pipeline

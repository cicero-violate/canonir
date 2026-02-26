# PROJECT_OVERVIEW.md


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

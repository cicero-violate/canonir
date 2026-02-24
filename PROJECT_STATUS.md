# PROJECT_STATUS.md

## Current State

- Workspace compiles clean.
- All IR gaps closed: E1–E15 (E9 lifetime nodes, E11 generic defaults included).
- Solvers active: S9 (borrow), S11 (const), S12 (macro), S13 (exhaustiveness), S15 (unsafe).
- S1 (transitive re-exports), S2 (SCC diag nodes), S4 (impl target hard error) closed.
- 8 CSR graphs wired end-to-end.
- Emitters cover all NodeKind variants.
- model_diff covers all graphs + emit_order + edge_hints.

## What Is Working

- Graph build → solve → validate → emit loop.
- Deterministic emit_order via stability_solver.
- Const cycle detection (S11).
- Macro recursion detection (S12).
- Unsafe caller warnings (S15).
- Enum exhaustiveness warnings (S13).
- Borrow solver: cycle detection on G_region via outlives_cycles (S9).
- Named / tuple / unit struct emission (E7).
- Inline module blocks (E10).
- Glob imports and pub use re-exports (E3/E15).
- extern crate declarations (E4).
- impl Trait / dyn Trait edges routed to type_graph (E8).
- Lifetime nodes + &'a T param round-trip (E9).
- GenericParam default_ty: T = Default emission (E11).
- Transitive Resolves chain following through Use nodes (S1).
- SCC cycle TypeRef diagnostic nodes injected into IR + emit_order (S2).
- Impl target validation accepts Struct/Enum/Trait/TypeAlias (S4).

## Remaining Gaps

| Gap | Missing                               |
|-----+---------------------------------------|
| S16 | drop_solver — blocked by ownership IR |

## Next Highest Value

1. capture_rustc round-trip closure (real .rs → capture → emit → identical .rs)
2. Full mutation test: AddNode + AddEdge + RemoveNode + diff_report.json
3. drop_solver / ownership IR (S16)

System invariant:
IR → Graph → Solve → Emit is stable.

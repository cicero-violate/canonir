# PROJECT_STATUS.md

## Current State

- Workspace builds; some warnings remain (lint/unused).
- All IR gaps closed: E1–E15 (E9 lifetime nodes, E11 generic defaults included).
- Solvers active: S9 (borrow), S11 (const), S12 (macro), S13 (exhaustiveness), S15 (unsafe), S16 (drop).
- 8 CSR graphs wired end-to-end.
- Projection pipeline: layout (passes) → emit (pure rendering). Emit layer contains no traversal/sorting/mutation.
- Layout ordering now explicit via `passes/order_items.rs` using NodeKind priority ExternCrate→Use→TypeAlias→Const→Static→Struct→Enum→Trait→Impl→Fn.
- model_diff covers all graphs + emit_order + edge_hints.

## What Is Working

- Graph build → solve → validate → emit loop.
- Deterministic emit_order via stability_solver + layout order_items pass (emit is dumb renderer).
- Const cycle detection (S11).
- Macro recursion detection (S12).
- Unsafe caller warnings (S15).
- Enum exhaustiveness warnings (S13).
- Borrow solver: cycle detection on G_region via outlives_cycles (S9).
- Named / tuple / unit struct emission (E7).
- Inline module blocks (E10) emitted without DFS traversal.
- Glob imports and pub use re-exports (E3/E15).
- extern crate declarations (E4).
- impl Trait / dyn Trait edges routed to type_graph (E8).
- Lifetime nodes + &'a T param round-trip (E9).
- GenericParam default_ty: T = Default emission (E11).
- Transitive Resolves chain following through Use nodes (S1).
- SCC cycle TypeRef diagnostic nodes injected into IR + emit_order (S2).
- Impl target validation accepts Struct/Enum/Trait/TypeAlias (S4).

## Next Highest Value

1. capture_rustc round-trip closure (real .rs → capture → emit → identical .rs)
2. Full mutation test: AddNode + AddEdge + RemoveNode + diff_report.json
3. Layout pass coverage/tests: per-pass unit tests + ordering guarantees
4. drop_solver ownership IR extension: scope nodes, conditional drop paths (S16b)

System invariant:
IR → Graph → Solve → Emit is stable.
- Drop order verification via post_dominators (S16).
- algorithms::control_flow::dominators extended with post_dominators().

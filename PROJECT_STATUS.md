# PROJECT_STATUS.md

## Current State

- Workspace compiles clean.
- All IR gaps E1/E2/E3/E4/E5/E6/E7/E8/E10/E12/E13/E14/E15 closed.
- Solvers S11/S12/S13/S15 activated.
- 8 CSR graphs wired end-to-end.
- Emitters cover all NodeKind variants including ExternCrate.
- model_diff covers all graphs + emit_order + edge_hints.

## What Is Working

- Graph build → solve → validate → emit loop.
- Deterministic emit_order via stability_solver.
- Const cycle detection.
- Macro recursion detection.
- Unsafe caller warnings.
- Enum exhaustiveness warnings.
- Named / tuple / unit struct emission (E7).
- Inline module blocks (E10).
- Glob imports and pub use re-exports (E3/E15).
- extern crate declarations (E4).
- impl Trait / dyn Trait edges routed to type_graph (E8).

## Remaining Gaps

| Gap | Missing                                                     |
|-----+-------------------------------------------------------------|
| E9  | Lifetime nodes + Outlives edges — blocks borrow_solver (S9) |
| E11 | Trait bounds round-trip validation on impl generics         |

## Remaining Solver Gaps

| Gap | Missing                                  |
|-----+------------------------------------------|
| S1  | Transitive re-export resolution          |
| S2  | Diagnostic node emission for SCC cycles  |
| S4  | Hard error for invalid impl targets      |
| S9  | borrow_solver — blocked by E9            |
| S16 | drop_solver — blocked by ownership IR    |

## Next Highest Value

1. Lifetime IR (E9) — unblocks borrow_solver
2. capture_rustc round-trip closure (real .rs → capture → emit → identical .rs)
3. Full mutation test: AddNode + AddEdge + RemoveNode + diff_report.json

System invariant:
IR → Graph → Solve → Emit is stable.

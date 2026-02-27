# Agent State

## 2026-02-27 — Current Cycle (Continue Plan: Phase 4.5)

### 1) Investigate the problem
- Continue Phase 4 after removing use/visibility solver compensation.
- Targets this cycle:
  - Phase 4.5: ensure `impl_solver` and `trait_solver` consume `ImplRef` from `G_type`.

### 2) Gather facts
- Capture emits `ImplRef` as `impl -> trait`, and assembly now routes `ImplRef` to `type_graph`.
- `impl_solver` previously validated only node fields (`Impl.for_trait`) and did not read `type_graph`.
- `trait_solver` previously used `Impl.for_trait` only and did not validate graph consistency.

### 3) Break down the facts
- Solvers should treat `type_graph` `ImplRef` as canonical relation, with node fields as cross-check.
- Add warnings when graph/field diverge or when expected graph edges are missing.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: `ImplRef` relation is read from `type_graph`.
- Structural pattern B: impl field vs graph relation mismatches are diagnostics.
- Categorical pattern A: graph-source alignment in impl/trait solvers.
- Categorical pattern B: consistency validation between node payload and graph edges.

### 6) Write it to state file
- The patterns above define this cycle's acceptance criteria.

### 7) Solve the state file
- `canon-analyzer/src/solver/impl_solver.rs`
  - added `type_graph` `ImplRef` walk per `Impl` node.
  - validates edge presence and warns on:
    - missing ImplRef for trait impls,
    - unexpected ImplRef on inherent impls,
    - multiple ImplRef edges,
    - mismatch between `Impl.for_trait` and graph target.
- `canon-analyzer/src/solver/trait_solver.rs`
  - added `type_graph` `ImplRef` map and consumes it as primary trait relation.
  - falls back to `Impl.for_trait` when graph edge is missing, while warning.
  - warns on multiple graph trait targets and field/graph mismatches.
  - keeps existing trait-method completeness checks.

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed after impl/trait solver changes.

### 9) Repeat step 3
- Post-change fact breakdown:
  - impl/trait reasoning now uses `ImplRef` from `G_type` as intended by routing change.
  - field/graph inconsistencies are surfaced as warnings rather than silently ignored.
- Next pending slice:
  - continue Phase 3.5 body structuring to further reduce `CfgOp::Raw`,
  - continue Phase 4.6 for `Instantiates` derivation in `G_type`.

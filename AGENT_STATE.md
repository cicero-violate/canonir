# Agent State

## 2026-02-27 — Current Cycle (Continue Plan: Phase 4.6)

### 1) Investigate the problem
- Continue Phase 4 after `ImplRef` graph-consumption updates.
- Targets this cycle:
  - Phase 4.6: derive `Instantiates` edges in `G_type` for generic `Extern`/`Unresolved` types that resolve to known canonical defs.

### 2) Gather facts
- `EdgeKind::Instantiates` exists in schema but had no analyzer derivation path.
- `type_solver` was effectively no-op (SCC scan only) and did not mutate `type_graph`.
- `TypeKind::Extern/Unresolved` generic arguments remain encoded in path text, so derivation requires parsing path generics.

### 3) Break down the facts
- For each `Type` node of `Extern/Unresolved` with generic args:
  - parse root + args from `path`.
  - if root resolves uniquely to local canonical def (Struct/Enum/Trait/TypeAlias), emit `Instantiates` edge to that def.
  - resolve each generic arg to unique canonical type node when possible and emit `Instantiates` to arg type nodes.
- Preserve existing `type_graph` edges and append derived edges without duplication.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: instantiation relations belong in `G_type`, not projection heuristics.
- Structural pattern B: derivation is additive and deduplicated.
- Categorical pattern A: generic path parsing in solver.
- Categorical pattern B: local def/type resolution for edge targets.

### 6) Write it to state file
- The patterns above define this cycle's acceptance criteria.

### 7) Solve the state file
- `canon-analyzer/src/solver/type_solver.rs`
  - added `derive_instantiates_edges(ir)` and integrated it into `solve`.
  - implemented generic path parsing helpers:
    - `split_generic_path`, `split_top_level`, `normalize_type_text`.
  - implemented type-key mapping helpers for matching args to canonical type nodes:
    - `type_kind_text_key`, `primitive_name`.
  - derivation behavior:
    - scans `TypeKind::Extern` and `TypeKind::Unresolved` nodes,
    - resolves generic root to unique local def by name where possible,
    - resolves arg types to unique canonical type nodes where possible,
    - appends deduped `EdgeKind::Instantiates` edges to `type_graph`.

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed after type-solver instantiation derivation changes.

### 9) Repeat step 3
- Post-change fact breakdown:
  - `G_type` now gets explicit `Instantiates` edges for the resolvable subset of generic extern/unresolved types.
  - unresolved/ambiguous cases remain non-fatal and are skipped (no synthetic guessing).
- Next pending slice:
  - continue Phase 3.5 body structuring to further reduce `CfgOp::Raw`,
  - move into Phase 5 projection heuristic removals/validation.

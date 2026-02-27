# Agent State

## 2026-02-27 — Current Cycle (schema-level instantiation invariant)

### 1) Investigate the problem
- The remaining structural gap was `type_solver` deriving `Instantiates` by splitting type-path text.

### 2) Gather facts
- `type_solver` used string helpers (`split_generic_path`, `split_top_level`, `normalize_type_text`).
- Capture had no explicit canonical shape for generic application roots + args.
- Dependency package invariant was already completed in prior cycle (`declared_dependencies` + `dependency_packages`).

### 3) Break down the facts
- Without an explicit applied-type representation, solver is forced into text parsing.
- Correct invariant: represent applied types structurally and derive `Instantiates` from that structure.

### 4) Write it to a state file
- This file is overwritten with the new cycle state.

### 5) Sort structural and categorical patterns
- Structural pattern A: heuristic code exists where the schema does not encode decomposition.
- Structural pattern B: once decomposition is in schema (`base`, `args`), solver logic becomes direct graph derivation.

### 6) Write it to state file
- Implemented invariants:
  - `TypeKind::Applied { base: CanonId, args: Vec<CanonId> }`
  - `TypeExpr::AppliedPath { base: String, args: Vec<TypeExpr> }` in capture model
  - `lower_ty` emits `AppliedPath` for ADT types with generic args
  - canon assembly lowers `AppliedPath` into `TypeKind::Applied`
  - projection renders `TypeKind::Applied` directly
  - analyzer `type_solver` now derives `Instantiates` from `Applied`, without text parsing

### 7) Solve the state file
- Removed string-based `Instantiates` derivation from `type_solver`.
- Replaced it with structural derivation:
  - applied type -> generic def (when resolvable structurally)
  - applied type -> each arg type

### 8) Emit and project the solution incrementally
- Validation:
  - workspace `cargo check` passes
  - `run_capture.sh` on `capture/repomap` passes
  - `cargo run -p orchestration -- <input> <output>` for repomap passes
  - emitted `emit/repomap` `cargo build` passes

### 9) Repeat step 3
- Remaining pending plan work:
  - Phase 3.5: reduce remaining body-level `CfgOp::Raw` emission for method/field/struct literal surfaces.
  - Documentation/status cleanup still listed in `EXECUTION_STATUS.md` (ownership/boundary docs).

# Agent State

## 2026-02-27 — Current Cycle (Continue Plan: Phase 3.5/3.7 then Phase 4.2)

### 1) Investigate the problem
- Continue from recent capture structure additions into the next pending slices.
- Targets this cycle:
  - Phase 3.5: reduce `CfgOp::Raw` by structurally lowering common body statements.
  - Phase 3.7: route assembly path interning through `CanonIR::intern_path`.
  - Phase 4.2: remove `dep_solver` text-scan fallback and heuristic root filtering.

### 2) Gather facts
- `Body::Raw` still dominated body capture output.
- `seal_body` previously collapsed each raw body to one `CfgOp::Raw`.
- `canon_assemble` still had direct `path_intern.intern(...)` call sites.
- `dep_solver` still had fallback scanning over `name_intern.vec` and `is_probable_crate_name`.

### 3) Break down the facts
- Full HIR/MIR lowering is larger than one increment, but partial structured lowering from raw statements is feasible now.
- Updated path interning should call `intern_path` at assembly boundaries.
- With `PathRef` present, dependency solving can drop text scanning and rely on structural nodes.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: convert recognisable raw statements into typed CFG ops.
- Structural pattern B: keep `CfgOp::Raw` as fallback only.
- Structural pattern C: call `canon.intern_path(...)` at assembly boundaries.
- Structural pattern D: dependency roots come from structural paths (`Use`, `PathRef`) only.
- Categorical pattern A: body-op lowering helpers in capture assembly.
- Categorical pattern B: path interning boundary cleanup.
- Categorical pattern C: dep solver fallback/heuristic removal.

### 6) Write it to state file
- The patterns above define this cycle's acceptance criteria.

### 7) Solve the state file
- `canon-capture/src/canon_assemble.rs`
  - added raw-body lowering helpers:
    - `synth_local`, `split_statements`,
    - `parse_method_call`, `parse_field_access`, `parse_index`, `parse_struct_lit`,
    - `lower_raw_stmt`, `lower_raw_body`.
  - changed `seal_body(Body::Raw)` to emit lowered ops instead of always a single `CfgOp::Raw`.
  - changed `Stmt::Raw` lowering to use `lower_raw_stmt`.
  - replaced direct `path_intern.intern(...)` with `canon.intern_path(...)` for:
    - `TypeKind::Extern` construction in `str_to_type_kind`,
    - `NodeKind::Module`, `Use`, `MacroCall`, `PathRef`,
    - extern-type normalization rewrite IDs.
  - fixed borrow conflict in extern-type normalization loop by switching to index-based iteration.
- `canon-analyzer/src/solver/dep_solver.rs`
  - removed raw-text fallback scan over `name_intern.vec`.
  - removed `is_probable_crate_name` heuristic filter.
  - dependency extraction now derives roots from structural `Use` + `PathRef` paths only, with builtin/local-root filtering retained.

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed after capture changes.
  - `cargo check` passed again after `dep_solver` fallback removal.

### 9) Repeat step 3
- Post-change fact breakdown:
  - `Body::Raw` now yields structural CFG ops for a subset of common patterns.
  - Remaining unsupported statement shapes still safely fall back to `CfgOp::Raw`.
  - Assembly path interning now consistently uses `intern_path` in updated call sites.
- `dep_solver` no longer compensates with text scanning; it consumes structural nodes only.
- Next pending slice:
  - continue reducing `CfgOp::Raw` dependence with additional structured body lowering,
  - continue Phase 4 compensation removal in `use_solver` and `visibility_solver`.

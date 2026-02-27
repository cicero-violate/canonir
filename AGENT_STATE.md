# Agent State

## 2026-02-27 — Current Cycle (Continue Plan: Phase 3.5 + 3.7)

### 1) Investigate the problem
- Continue Phase 3 with the next pending capture-side structural increment.
- Target this cycle: reduce `CfgOp::Raw` usage for body statements and move path interning call sites to `CanonIR::intern_path`.

### 2) Gather facts
- Body capture still emits `Body::Raw` for most function/method bodies.
- `seal_body` converted `Body::Raw` to a single `CfgOp::Raw`, losing structure for method calls, field access, index access, and struct literals.
- `canon_assemble` still had multiple direct `path_intern.intern(...)` call sites.

### 3) Break down the facts
- A full HIR/MIR body lowering is larger than one cycle, but we can incrementally parse raw statements and emit structured `CfgOp` where pattern-safe.
- Normalization authority should move to `intern_path` call sites to reduce ad-hoc path handling.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: convert recognisable raw statements into typed CFG ops.
- Structural pattern B: keep `CfgOp::Raw` as fallback only.
- Structural pattern C: call `canon.intern_path(...)` at assembly boundaries.
- Categorical pattern A: body-op lowering helpers in capture assembly.
- Categorical pattern B: path interning boundary cleanup.

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

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed for the workspace.

### 9) Repeat step 3
- Post-change fact breakdown:
  - `Body::Raw` now yields structural CFG ops for a subset of common patterns.
  - Remaining unsupported statement shapes still safely fall back to `CfgOp::Raw`.
  - Assembly path interning now consistently uses `intern_path` in updated call sites.
- Next pending slice:
  - continue reducing `CfgOp::Raw` dependence with additional structured body lowering,
  - then proceed into Phase 4 solver compensation removal (`use_solver`, `dep_solver`, `visibility_solver`).

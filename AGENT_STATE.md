# Agent State

## 2026-02-27 — Current Cycle (capture cleanup: remove dead raw parser fallback surface)

### 1) Investigate the problem
- `canon_assemble` still contained legacy raw-statement parser helpers (`parse_method_call`, `parse_field_access`, `parse_index`, `parse_struct_lit`) from pre-structural body handling.
- These helpers were no longer used by current body lowering flow and represented stale fallback/heuristic surface.

### 2) Gather facts
- Current body path extraction is already structural through MIR (`project/body.rs`) and `PathRef` node emission.
- Current body op lowering in assemble uses direct `Stmt` mapping and `CfgOp::Raw` only where source model is raw.
- The parser helpers had no active call sites in the current pipeline.

### 3) Break down the facts
- Remove dead fallback parser helpers and dependent utilities.
- Keep active lowering path unchanged.
- Revalidate full compile + orchestration pipelines.

### 4) Write it to a state file
- This file is the overwritten cycle snapshot.

### 5) Sort structural and categorical patterns
- Structural pattern A: body projection/lowering path is explicit and direct; no hidden parser branch.
- Structural pattern B: stale helper code removed to enforce one structural path.
- Categorical pattern A: reduce fallback/regression surface area.

### 6) Write it to state file
- Acceptance criteria:
  - dead raw parser helpers removed,
  - build and orchestration remain green.

### 7) Solve the state file
- `canon-capture/src/canon_assemble.rs`
  - removed unused functions:
    - `split_top_level`
    - `intern_type_path_expr`
    - `synth_local`
    - `split_statements`
    - `parse_method_call`
    - `parse_field_access`
    - `parse_index`
    - `parse_struct_lit`
    - `is_type_like_head`
  - kept active body lowering functions (`lower_raw_stmt`, `lower_raw_body`, `seal_body`) intact.

### 8) Emit and project the solution incrementally
- Validation:
  - `cargo check` passed.
  - capture + orchestration passed for:
    - `test_projects/test_rust_projects/capture/test_1`
    - `test_projects/test_rust_projects/capture/repomap`

### 9) Repeat step 3
- Post-change fact breakdown:
  - stale fallback parser code has been removed.
  - active structural body path remains stable.
- Next pending slice:
  - continue Phase 3.5/4 cleanup for remaining non-structural body modeling gaps (promote additional CFG/body structure where source model still uses raw text).

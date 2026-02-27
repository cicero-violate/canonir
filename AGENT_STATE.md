# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CAPTURE_REFACTOR_MODEL_EXECUTION_SLICE_27`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- `capture/mir/lower.rs` remained large due to embedded expression and statement-builder logic.

### 2) Gather facts
- The following builder cluster was still in `lower.rs`:
- projection renderer
- assign/rvalue lowering
- field-access lowering
- struct-literal lowering
- binop/unop token mapping

### 3) Break down the facts
- These functions are pure MIR expression/statement construction and do not require CFG-walker ownership.
- They can be moved behind a dedicated module boundary and called by the block loop.

### 4) Write it to a state file
- State overwritten for this slice.

### 5) Sort structural and categorical patterns
- Pattern A: dedicated module extraction (`capture/mir/expr.rs`).
- Pattern B: orchestrator thinning (`lower.rs` delegates to `mir_expr`).
- Pattern C: module boundary hardening (`mod.rs` exports new module).

### 6) Write it to state file
- Files changed:
- `canon-capture/src/capture/mir/expr.rs` (new)
- `canon-capture/src/capture/mir/lower.rs`
- `canon-capture/src/capture/mir/mod.rs`
- `STRUCTURAL_INVARIANTS_REPORT.md`
- `AGENT_STATE.md`

### 7) Solve the state file
- Added `capture/mir/expr.rs` with moved logic:
- `render_projected_place_expr`
- `mir_binop_token` / `mir_unop_token`
- `mir_assign_stmt` + `mir_rvalue_expr`
- `mir_field_access_stmt`
- `mir_struct_lit_stmt`
- Rewired `lower.rs` call sites to `mir_expr::...`.
- Removed duplicated moved implementations from `lower.rs`.
- Updated `capture/mir/mod.rs` to export `expr`.

### 8) Emit and project the solution incrementally
- Validation:
- `cargo check -p canon-capture`: pass.
- `./run_script.sh repomap`: pass.
- `STRUCTURAL_INVARIANTS_REPORT.md` regenerated.
- LOC snapshot:
- `capture/mir/lower.rs`: 758 LOC (down from 1079 in previous slice).

### 9) Repeat step 3
- Next structural slice:
- extract remaining utility/control helpers from `lower.rs` (`label_place_dest`, return/terminator helper cluster, local-use utilities) into focused modules so `lower.rs` approaches pure CFG traversal + dispatch.

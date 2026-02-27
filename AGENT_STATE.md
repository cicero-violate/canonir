# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CAPTURE_REFACTOR_MODEL_EXECUTION_SLICE_10`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Method/plain call statement builders still lived in `project/item.rs`, despite most call-label/filter helper dependencies already moved to MIR lower layer.

### 2) Gather facts
- Remaining functions were self-contained and depended on helpers now owned by `capture/mir/lower.rs`:
- `mir_method_call_stmt`,
- `mir_call_stmt`.

### 3) Break down the facts
- Migration action:
- move method/plain call stmt builders into `capture/mir/lower.rs`,
- import from new owner in `project/item.rs`,
- delete local duplicate definitions.

### 4) Write it to a state file
- State overwritten for this slice.

### 5) Sort structural and categorical patterns
- Pattern A: call-lowering ownership consolidation under MIR layer.
- Pattern B: incremental elimination of `project/item.rs` helper responsibilities.

### 6) Write it to state file
- Files changed:
- `canon-capture/src/capture/mir/lower.rs`
- `canon-capture/src/project/item.rs`
- `AGENT_STATE.md`

### 7) Solve the state file
- Added to `capture/mir/lower.rs`:
- `mir_method_call_stmt`,
- `mir_call_stmt`.
- Updated `project/item.rs` imports to consume these from `capture::mir::lower`.
- Removed local `mir_method_call_stmt` and `mir_call_stmt` definitions from `project/item.rs`.

### 8) Emit and project the solution incrementally
- Validation:
- `cargo check -p canon-capture`: pass.
- `./run_script.sh repomap`: pass.

### 9) Repeat step 3
- Next structural slice:
- migrate projected-place rendering helpers (`render_projected_place_expr`, binop/unop token helpers) to `capture/mir/lower.rs`,
- continue reducing `project/item.rs` to CFG orchestration + legacy bridge body only.

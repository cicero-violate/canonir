# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CAPTURE_REFACTOR_MODEL_EXECUTION_SLICE_35`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Stage boundaries existed at top-level (`build plan`, `emit`, `finalize`) but block emission internals were still monolithic.

### 2) Gather facts
- `stage_emit_blocks` mixed four responsibilities:
1. special block routing (switch source/arm suppression)
2. statement pre-seeding (suppressed sentinels)
3. statement lowering
4. terminator lowering and block assembly

### 3) Break down the facts
- Convert block emission to explicit sub-stages while freezing behavior:
- `stage_emit_special_block`
- `stage_prepare_block_stmts`
- `stage_lower_block_statements`
- `stage_lower_block_terminator`
- `stage_finalize_block`

### 4) Write it to a state file
- State overwritten for this slice.

### 5) Sort structural and categorical patterns
- Pattern A: stage decomposition (emit-phase internals).
- Pattern B: orchestration-first readability (main loop now dispatches stages).
- Pattern C: behavior freeze preserved (same solver/capture outputs).

### 6) Write it to state file
- Files changed:
- `canon-capture/src/capture/mir/lower.rs`
- `STRUCTURAL_INVARIANTS_REPORT.md`
- `AGENT_STATE.md`

### 7) Solve the state file
- Refactored `stage_emit_blocks` to call explicit sub-stage functions.
- Extracted special/suppressed/stmt/terminator/finalize responsibilities into dedicated functions.
- Preserved all existing structural decision points and derived-state checks.

### 8) Emit and project the solution incrementally
- Validation:
- `cargo check -p canon-capture`: pass.
- `./run_script.sh repomap`: pass.
- `STRUCTURAL_INVARIANTS_REPORT.md` regenerated.

### 9) Repeat step 3
- Mutation trend checkpoint:
- LOC did not decrease (`lower.rs` increased with stage scaffolding), confirming current phase is responsibility clarification.
- Behavior freeze remains active: next changes should focus on moving stage functions to dedicated modules/files without semantic changes.

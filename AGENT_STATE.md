# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CAPTURE_REFACTOR_MODEL_EXECUTION_SLICE_22`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- MIR lowering still had remaining inline logic for non-call terminators and partial assign suppression classification outside the pattern table.

### 2) Gather facts
- `lower_call_terminator(...)` already existed, but return/goto/drop/assert/switchint logic was still in the main loop.
- Zero-arg enum ctor suppression check was still ad-hoc after pattern dispatch.

### 3) Break down the facts
- Structural targets for this slice:
- extract non-call terminator lowering into dedicated helpers,
- move constant-use assignment class into `patterns.rs` dispatch domain.

### 4) Write it to a state file
- State overwritten for this slice.

### 5) Sort structural and categorical patterns
- Pattern A: terminator decomposition (`call` vs `non-call`).
- Pattern B: pattern-table expansion (`ConstUse` classification).

### 6) Write it to state file
- Files changed:
- `canon-capture/src/capture/mir/lower.rs`
- `canon-capture/src/capture/mir/patterns.rs`
- `STRUCTURAL_INVARIANTS_REPORT.md`
- `AGENT_STATE.md`

### 7) Solve the state file
- Added and wired:
- `lower_non_call_terminator(...)`
- `lower_return_terminator(...)`
- `remap_to_goto(...)`
- Main loop now delegates non-call terminators through helper dispatch.
- Added `MirOpKind::ConstUse` in `capture/mir/patterns.rs` and routed zero-arg enum ctor suppression through dispatcher path.
- Removed redundant post-dispatch zero-arg enum check from assign flow.

### 8) Emit and project the solution incrementally
- Validation:
- `cargo check -p canon-capture`: pass.
- `./run_script.sh repomap`: pass.
- `STRUCTURAL_INVARIANTS_REPORT.md` regenerated.

### 9) Repeat step 3
- Next structural slice:
- continue MIR compression by moving additional assignment-specialization tests from `lower.rs` into `patterns.rs` predicates and thin helper adapters,
- keep `lower.rs` focused on CFG traversal/orchestration only.

# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_STRUCTURAL_HARVEST_SLICE_12`
- date: `2026-02-27`
- mode: `execution`
- invariant: `Structural invariants only. No heuristics.`

### 1) Investigate the problem
- Objective: continue suppression-volume reduction while preserving `run_script.sh` build invariants.

### 2) Gather facts
- `run_script.sh test_1` passes `build_emit`.
- `run_script.sh repomap` passes `build_emit`.
- `test_1` suppression reduced from 11 -> 9 after structural fixes.

### 3) Break down the facts
- Pending Gap A: `results::combine_results` still collapses via suppressed closure/method-call chain.
- Pending Gap B: `core::engine::uses_dyn_trait` still suppresses return path under dynamic dispatch.
- Pending Gap C: repomap hotspot bodies (`extractor::*`, `symbol::render`) remain high-collapse.

### 4) Write it to a state file
- State overwritten for this execution slice (no append).

### 5) Sort structural and categorical patterns
- Pattern 1: enum unit-variant return paths can be emitted structurally (validated via `User::status`).
- Pattern 2: unresolved dynamic/closure call chains trigger destination sentinels and propagate suppression.

### 6) Write it to state file
- Active files for next slice:
- `canon-capture/src/capture/mir/lower.rs`
- `canon-capture/src/capture/mir/ops.rs`
- `canon-capture/src/capture/mir/guard.rs`

### 7) Solve the state file
- Completed this slice:
- `StructLit` fallback now correctly falls through to generic assign lowering.
- Added unit-ADT aggregate expression lowering in MIR rvalue emission.
- Excluded cleanup edges from switch analysis graph construction.

### 8) Emit and project the solution incrementally
- Validation executed:
- `/workspace/ai_sandbox/canon/run_script.sh test_1`
- `/workspace/ai_sandbox/canon/run_script.sh repomap`

### 9) Repeat step 3
- Next structural target:
- capture `combine_results` closure/map_err chain without destination suppression.
- capture dynamic-dispatch method return (`uses_dyn_trait`) without fallback sentinel.

# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_STRUCTURAL_HARVEST_SLICE_10`
- date: `2026-02-27`
- mode: `execution`
- invariant: `Structural invariants only. No heuristics.`

### 1) Investigate the problem
- Objective: continue structural capture invariants and remove remaining emission-side structural corruption after return-carrier closure.

### 2) Gather facts
- `canon-capture` builds cleanly (`cargo check -p canon-capture`).
- Repomap capture+orchestration succeeds.
- Repomap unresolved `__ret` sentinel sites are now zero.
- Remaining repomap build failures are concentrated in fmt/argument materialization and tuple-shape mismatches in `fn_signature` and `symbol::render` paths.

### 3) Break down the facts
- Gap Class A: fmt internals leakage (`std::fmt::Arguments::new`, rt `Argument` tuple/array forms) still emitted.
- Gap Class B: projection/body rendering still emits tuple-shape-incompatible argument carriers in `symbol::render`.
- Gap Class C: deref trait-call forms (`Vec::deref`) still leak as method-like calls without structural lowering.

### 4) Write it to a state file
- State overwritten for this execution slice.

### 5) Sort structural and categorical patterns
- Return-carrier invariant is now structurally satisfied for repomap (`__ret` unresolved count = 0).
- Next invariant boundary is fmt/rt argument construction elimination from capture output.

### 6) Write it to state file
- Files touched this slice:
- `canon-capture/src/capture/mir/analysis.rs`
- `canon-capture/src/capture/mir/passes.rs`
- `canon-capture/src/capture/mir/filters.rs`
- `canon-capture/src/capture/mir/ops.rs`
- `canon-capture/src/capture/mir/terminator.rs`
- `AGENT_STATE.md`

### 7) Solve the state file
- Implemented structural cycle detection for switch regions (replaced backedge heuristic).
- Preserved return-carrying switch-arm blocks (write-return and return-terminator arms are no longer suppressed).
- Removed switch-source synthetic `Match{dest:__ret}` injection.
- Added structural `must_use` identity-call lowering to preserve return value flow.
- Expanded format-target detection and fmt constructor filtering shape.

### 8) Emit and project the solution incrementally
- Validation executed:
- `cargo check -p canon-capture`
- `run_capture.sh .../capture/repomap .../canon_capture.json`
- `cargo run -p orchestration -- .../canon_capture.json .../emit/repomap`
- `cargo build` in `emit/repomap`

### 9) Repeat step 3
- Next structural target:
- suppress/lower fmt runtime argument constructors (`core::fmt::rt::Argument::*`, `Arguments::new`) and dependent tuple carriers in capture so emitter receives only structural calls/assigns.

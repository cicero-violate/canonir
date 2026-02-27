# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_STRUCTURAL_HARVEST_SLICE_03`
- date: `2026-02-27`
- mode: `execution`
- invariant: `Structural invariants only. No heuristics.`

### 1) Investigate the problem
- Remaining structural surface after prior slices: `match gap`, `unreachable`, `//goto`.

### 2) Gather facts
- `run_script.sh repomap` metrics became the canonical extraction source.
- Primary emission hotspots were in MIR normalization and projection body rendering.

### 3) Break down the facts
- Category A: synthetic control-flow artifacts emitted as source (`Goto`, `Branch`, `Unreachable`).
- Category B: `Match { dest: Some(__ret) }` placeholders creating `canon match result not lowered`.
- Category C: suppression over-injection volume.

### 4) Write it to a state file
- State overwritten for this execution slice.

### 5) Sort structural and categorical patterns
- Structural pass pattern: normalize body ops first, then prune unreachable source-level artifacts.
- Return invariant pattern: keep `__ret` binding structurally present for non-unit completeness.

### 6) Write it to state file
- Files touched in this slice:
- `canon-capture/src/capture/mir/passes.rs`
- `canon-projection/src/emit/body.rs`
- `run_script.sh`
- `STRUCTURAL_INVARIANTS_REPORT.md`

### 7) Solve the state file
- Added MIR normalization pass composition:
- lower `Stmt::Match { dest: Some(x) }` -> `Stmt::Assign { lhs: x, rhs: "__canon_suppressed__" }`
- prune suppressed bindings only when truly unused, but never prune `lhs == "__ret"`.
- Projection invariant:
- do not emit source lines for CFG metadata ops (`Branch`, `Goto`, `Unreachable`).

### 8) Emit and project the solution incrementally
- Validation executed:
- `cargo check -p canon-capture -p canon-analyzer -p canon-projection -p orchestration`
- `./run_script.sh repomap`
- Current repomap structural surface:
- `canon suppressed binding count: 13`
- `canon match gap count: 0`
- `unreachable count: 0`
- `// match count: 0`
- `// goto count: 0`

### 9) Repeat step 3
- Next structural target:
- reduce suppressed-binding count (`13`) via additional capture invariants that produce renderable statements instead of suppression.

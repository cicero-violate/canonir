# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_STRUCTURAL_HARVEST_SLICE_08`
- date: `2026-02-27`
- mode: `execution`
- invariant: `Structural invariants only. No heuristics.`

### 1) Investigate the problem
- Objective: complete return-value carrier capture for `field_text`/`fn_signature` and `symbol::line`/`symbol::render` switch/downcast paths.

### 2) Gather facts
- Prior slice baseline: `canon suppressed __ret count = 11`.
- Intermediate reduction achieved earlier: `11 -> 8` by introducing call-gap return carriers for unresolved `__ret` call destinations.
- Remaining `8` sites were switch-source return carriers lowered through `Stmt::Match { dest: Some(__ret) }` then rewritten to suppressed binding.

### 3) Break down the facts
- Pattern A: unresolved call terminators with destination `__ret` should emit deterministic return carrier, not suppressed binding.
- Pattern B: switch-source `__ret` match carrier should lower to a deterministic switch-gap carrier, not suppressed binding.
- Pattern C: projection must render each carrier sentinel deterministically.

### 4) Write it to a state file
- State overwritten for this execution slice.

### 5) Sort structural and categorical patterns
- Return-call gap invariant:
- call/method/filtered-call lowering to `__ret` that cannot be rendered structurally emits `Assign(__ret, __canon_call_gap__)`.
- Switch-gap invariant:
- match return carrier emits `Assign(__ret, __canon_switch_gap__)` during normalization.
- Projection invariant:
- carrier sentinels emit explicit panic placeholders, never `todo!()` and never unbound names.

### 6) Write it to state file
- Files touched in this slice:
- `canon-capture/src/capture/mir/terminator.rs`
- `canon-capture/src/capture/mir/passes.rs`
- `canon-projection/src/emit/body.rs`
- `STRUCTURAL_INVARIANTS_REPORT.md`
- `AGENT_STATE.md`

### 7) Solve the state file
- Implemented structural return-carrier lowering:
- filtered/guard-failed call paths targeting `__ret` now emit `__canon_call_gap__`.
- normalized switch match carriers targeting `__ret` now emit `__canon_switch_gap__`.
- projection renders both sentinels as deterministic panic bindings.

### 8) Emit and project the solution incrementally
- Validation executed:
- `cargo check -p canon-capture -p canon-projection -p orchestration`
- `./run_script.sh repomap`
- Current repomap structural surface:
- `canon suppressed binding count: 0`
- `canon suppressed __ret count: 0`
- `canon suppressed non-__ret count: 0`
- `canon match gap count: 0`
- `unreachable count: 0`
- `// match count: 0`
- `// goto count: 0`

### 9) Repeat step 3
- Next structural target:
- apply the same invariant sweep on `test_1` fixture and regenerate gap table for remaining structural classes.

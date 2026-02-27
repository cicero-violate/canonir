# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_STRUCTURAL_HARVEST_SLICE_06`
- date: `2026-02-27`
- mode: `execution`
- invariant: `Structural invariants only. No heuristics.`

### 1) Investigate the problem
- Remaining structural target: reduce `__ret`-only suppressed return carriers.

### 2) Gather facts
- After slice 05, non-`__ret` suppression reached zero.
- Residual surface is entirely `__ret` suppression in 12 concrete functions.

### 3) Break down the facts
- Category A: unresolved return-value capture in concrete functions.
- Category B: need function-level site mapping to drive the next lowering pass.

### 4) Write it to a state file
- State overwritten for this execution slice.

### 5) Sort structural and categorical patterns
- Harvest pattern: classify structural gaps by role before changing lowering.
- Metric pattern: keep aggregate and per-class counts in one report.

### 6) Write it to state file
- Files touched in this slice:
- `run_script.sh`
- `STRUCTURAL_INVARIANTS_REPORT.md`
- `AGENT_STATE.md`

### 7) Solve the state file
- Enhanced `run_script.sh` emitted-surface extraction:
- now records exact function-site list for each `__ret` suppressed binding
- function detection matches both `pub fn` and indented `fn` signatures.

### 8) Emit and project the solution incrementally
- Validation executed:
- `cargo check -p canon-projection -p orchestration`
- `./run_script.sh repomap`
- Current repomap structural surface:
- `canon suppressed binding count: 12`
- `canon suppressed __ret count: 12`
- `canon suppressed non-__ret count: 0`
- `canon match gap count: 0`
- `unreachable count: 0`
- `// match count: 0`
- `// goto count: 0`

### 9) Repeat step 3
- Next structural target:
- implement return-value structural capture on the listed 12 functions/patterns, starting with smallest patterns (`symbol::line`, `symbol::render`, `node_text` family).

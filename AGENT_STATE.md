# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_STRUCTURAL_HARVEST_SLICE_11`
- date: `2026-02-27`
- mode: `execution`
- invariant: `Structural invariants only. No heuristics.`

### 1) Investigate the problem
- Objective: continue structural invariant harvesting while keeping both fixture pipelines buildable.

### 2) Gather facts
- `run_script.sh test_1` passes `build_emit`.
- `run_script.sh repomap` passes `build_emit`.
- Structural output still has high suppression density and large semantic drift versus fixture sources.

### 3) Break down the facts
- Gap Class A: over-suppression in function bodies (`panic!(\"canon suppressed binding\")`) reduces semantic fidelity.
- Gap Class B: unresolved semantic reconstruction in complex functions (`symbol::render`, extractor loops) despite structural compilability.
- Gap Class C: projection/call lowering remains conservative, causing large body collapse.

### 4) Write it to a state file
- State overwritten for this execution slice (no append).

### 5) Sort structural and categorical patterns
- Structural compile invariants are currently satisfied for both fixtures.
- Next phase is controlled suppression reduction with invariant-preserving re-expansion of body ops.

### 6) Write it to state file
- Files touched this slice:
- `canon-capture/src/capture/mir/analysis.rs`
- `canon-capture/src/capture/mir/expr.rs`
- `canon-capture/src/capture/mir/filters.rs`
- `canon-capture/src/capture/mir/lower.rs`
- `canon-capture/src/capture/mir/ops.rs`
- `canon-capture/src/capture/mir/terminator.rs`
- `canon-projection/src/emit/body.rs`
- `AGENT_STATE.md`

### 7) Solve the state file
- Added destination sentinel emission for field/struct lowering misses.
- Tightened projection rendering invariants to suppress invalid/private field forms.
- Added structural filtering for fmt internals and unresolved-generic call paths.
- Added associated-call canonicalization to emit `Type::method` for impl-associated calls.

### 8) Emit and project the solution incrementally
- Validation executed:
- `/workspace/ai_sandbox/canon/run_script.sh test_1`
- `/workspace/ai_sandbox/canon/run_script.sh repomap`

### 9) Repeat step 3
- Next structural target:
- reduce suppression volume structurally for top semantic hotspots (`extractor::*`, `symbol::render`, `results::combine_results`) while preserving compile invariants.

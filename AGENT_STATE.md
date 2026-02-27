# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_CAPTURE_LOC_REDUCTION_V1_PHASE_7_SWEEP_AND_VISPATH_INVARIANT`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Run broader validation sweep and resolve structural blockers revealed outside the small-fixture baseline.

### 2) Gather facts
- `run_script.sh` is path-fragile (invokes `./run_capture.sh` after `cd` into emit dirs).
- Expanded fixtures initially failed capture with:
- panic: `invalid path for path_intern` at `canon/src/ir.rs:263`
- backtrace source: `canon_capture::canon_assemble` pending visibility path interning (`canon-capture/src/canon_assemble.rs:750`).
- Structural fix applied in `map_vis`:
- `Visibility::PubIn` now only emitted for non-empty canonical paths.
- empty canonical path degrades to `Visibility::Private`
- canonical `"crate"` degrades to `Visibility::PubCrate`

### 3) Break down the facts
- The capture panic was an invariant breach (`PubIn` with empty path payload), now fixed.
- After the fix:
- `conversation` capture succeeds (17212 nodes)
- `semantic-lint` capture succeeds (94764 nodes)
- New gate failures are analyzer-level return completeness:
- conversation: `return_solver` fails non-unit `visit_map` (node 419)
- semantic-lint: `return_solver` fails non-unit `visit_str` (node 4897)
- `canon` fixture: orchestration succeeds; emitted cargo build fails offline dependency fetch (`canon v0.1.1`).

### 4) Write it to a state file
- State overwritten to current checkpoint.

### 5) Sort structural and categorical patterns
- Pattern A: visibility path canonicalization must happen before assembly interning.
- Pattern B: large-fixture failures now concentrate in return completeness for non-unit functions.
- Pattern C: `emit/canon` build failure is environment/dependency mode (`--offline`) rather than structural capture/analyzer failure.

### 6) Write it to state file
- Files changed this slice:
- `canon-capture/src/project/helpers.rs`
- `canon-capture/src/project/body.rs`
- `canon-capture/src/project/edge_emit.rs`
- `canon-capture/src/project/engine.rs`
- `canon-capture/src/project/relations.rs`
- `canon-capture/src/project/mod.rs`
- `PLAN.md`
- `AGENT_STATE.md`
- `PROJECT_STATUS.md`

### 7) Solve the state file
- Completed structural fix for empty visibility path invariant.
- Completed project-side edge emission unification.
- Phase 7 broad sweep now has concrete next structural target: return solver gaps for expanded fixtures.

### 8) Emit and project the solution incrementally
- Validation performed:
- `cargo check -p canon-capture`: pass
- `cargo check` workspace: pass
- `repomap` capture/orchestration/emitted build: pass
- `test_1` capture/orchestration/emitted build: pass
- `conversation` capture: pass; orchestration: fail (`return_solver visit_map`)
- `semantic-lint` capture: pass; orchestration: fail (`return_solver visit_str`)
- `canon` capture/orchestration: pass; emitted build: fail due offline crate fetch

### 9) Repeat step 3
- Next slice:
- diagnose MIR/structured-body shape for failing `visit_map` and `visit_str`
- implement structural return completeness fixes (no heuristics, no raw fallback)

# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_CAPTURE_LOC_REDUCTION_V1_PHASE_7_RETURN_INVARIANT_SWEEP2`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Resolve expanded-fixture return-solver failures after the broad sweep.

### 2) Gather facts
- Failing analyzer cases were:
- `conversation`: `return_solver` non-unit `visit_map` terminal block without return-producing op.
- `semantic-lint`: `return_solver` non-unit `visit_str` terminal block without return-producing op.
- Root shape in capture:
- switch-suppressed source/arm blocks were emitted with `Terminator::None` and often empty ops.
- Return solver treats reachable terminal blocks without return-producing ops as invariant violations.

### 3) Break down the facts
- Suppressed switch regions are intentionally non-executable in the reduced structured body model.
- Therefore their terminal representation must be structurally terminal (`Unreachable`), not silent `None`.

### 4) Write it to a state file
- State overwritten to current checkpoint.

### 5) Sort structural and categorical patterns
- Pattern A: switch-source suppression block must be terminalized.
- Pattern B: switch-arm suppression block must be terminalized.
- Pattern C: return completeness solver invariant aligns with `Unreachable` terminal semantics.

### 6) Write it to state file
- Files changed this slice:
- `canon-capture/src/project/item.rs`
- `canon-capture/src/project/helpers.rs`
- `run_script.sh`
- `PLAN.md`
- `AGENT_STATE.md`
- `PROJECT_STATUS.md`

### 7) Solve the state file
- Implemented structural fix:
- in `mir_body_structural`, switch-suppressed source and arm blocks now emit `Terminator::Unreachable` (instead of `Terminator::None`).
- Also fixed visibility invariant from prior slice:
- `map_vis` no longer emits empty `Visibility::PubIn(path)`.
- Script invariant fix:
- `run_script.sh` now resolves `run_capture.sh` via script directory, not current working directory.

### 8) Emit and project the solution incrementally
- Validation performed:
- `cargo check -p canon-capture`: pass
- `cargo check` workspace: pass
- `conversation` capture + orchestration: pass
- `semantic-lint` capture + orchestration: pass
- `repomap` capture/orchestration/build: pass
- `test_1` capture/orchestration/build: pass
- `canon` capture + orchestration: pass
- Remaining build blockers:
- `emit/conversation`, `emit/semantic-lint`, `emit/canon` fail in offline mode due missing crates.io dependencies.

### 9) Repeat step 3
- Next slice:
- classify offline dependency blockers separately from structural pipeline status
- continue Phase 7 gate reporting with explicit structural-vs-environment split

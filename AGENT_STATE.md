# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_BODY_RETURN_INVARIANTS_V1`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Goal continuation: reduce non-unit return fallback dependence by improving structural MIR return reconstruction.

### 2) Gather facts
- Raw body/op variants are already removed from active model path.
- Remaining gap appears as emitted non-unit `todo!()` fallback sites.
- Fixture metric after this cycle remains `13` `todo!()` sites per fixture (`repomap`, `test_1`).

### 3) Break down the facts
- Missing return completeness is driven by unsupported MIR constructs in some functions (e.g., clone bodies that still lower to goto-heavy blocks with no value-return op).
- Assignment capture needs strict declaration/value guards to avoid unresolved RHS emission.

### 4) Write it to a state file
- File overwritten to current cycle state.

### 5) Sort structural and categorical patterns
- Pattern A: adding `Assign` capture improves value flow only when RHS identities are already structurally known.
- Pattern B: generic call capture is required beyond method-only call capture.
- Pattern C: strict fail-fast invariant for missing non-unit return ops is not yet deployable without broader MIR lowering coverage.

### 6) Write it to state file
- Implemented in this cycle:
  - Added `Stmt::Assign` and lowering to `CfgOp::Assign`.
  - Added guarded assignment emission based on known-defined value set.
  - Added `Stmt::Call` and generic call lowering path (non-method MIR calls).
  - Added enum-constructor handling in struct-literal lowering path (variant path capture).
  - Projection `bind_or_assign` now introduces mutable bindings for reassignment safety.
- Attempted strict return invariant enforcement and projection fallback removal; reverted to keep pipeline operational while return coverage is incomplete.

### 7) Solve the state file
- Slice outcome: structural return/value capture improved without regressing fixture builds.
- Remaining unresolved: eliminate non-unit `todo!()` fallback by completing return-value reconstruction coverage.

### 8) Emit and project the solution incrementally
- Validation:
  - workspace `cargo check`: pass
  - `repomap` capture -> orchestration -> emitted build: pass
  - `test_1` capture -> orchestration -> emitted build: pass
  - emitted `todo!()` count metric: `13` in `repomap`, `13` in `test_1`

### 9) Repeat step 3
- Next work:
  - expand MIR lowering for currently unrepresented return-producing constructs,
  - then remove projection-side non-unit `todo!()` injection and re-enable strict non-unit return invariant.

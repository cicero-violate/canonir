# AGENT_STATE.md

## CANONICAL_HEADER
- state_id: `CANON_BODY_RETURN_INVARIANTS_V1`
- date: `2026-02-27`
- mode: `execution`
- invariant: `No heuristics. Structural invariants only.`

### 1) Investigate the problem
- Continue advancing Canon goal after raw body/op variant removal.
- Next blocker: non-unit return reconstruction is still incomplete.

### 2) Gather facts
- MIR body path is primary and raw variants are removed.
- Added structural `Stmt::Assign` capture and `CfgOp::Assign` lowering.
- Introduced canonical return-place local mapping (`_0 -> __ret`) and guarded assignment emission.
- Emitted fixtures still contain `todo!()` in non-unit functions.

### 3) Break down the facts
- Remaining gap is return-value completeness, not raw-op fallback.
- Projection still injects `todo!()` for non-unit functions missing explicit return structure.
- This is the next structural invariant target.

### 4) Write it to a state file
- File overwritten with current cycle state.

### 5) Sort structural and categorical patterns
- Pattern A: assignment capture without declaration invariants causes undefined RHS values.
- Pattern B: guarded assignment emission avoids invalid code but leaves return gaps.
- Pattern C: return completeness must be solved in capture/IR, not projection fallback.

### 6) Write it to state file
- Completed in this slice:
  - Added `Stmt::Assign`.
  - Lowered `Stmt::Assign` -> `CfgOp::Assign`.
  - Added assignment-definition guard in MIR body projection.
- Pending:
  - remove projection-side non-unit `todo!()` injection after return structure is complete.

### 7) Solve the state file
- Current slice solved assignment-level local/value invariant tightening.
- Return invariant remains active and is now the explicit plan focus.

### 8) Emit and project the solution incrementally
- Validation:
  - workspace `cargo check`: pass.
  - repomap pipeline build: pass.
  - emitted `todo!()` instances remain (tracked as current structural gap metric).

### 9) Repeat step 3
- Execute `PLAN.md` `PHASE_R1_RETURN_VALUE_CAPTURE` next until `todo!()` return fallback is removable.

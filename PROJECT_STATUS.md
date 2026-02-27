# PROJECT_STATUS.md

## CANONICAL_HEADER
- project: `canon`
- status_epoch: `2026-02-27`
- pipeline_invariant: `Capture -> CanonIR -> Graph -> Solve -> Emit`
- policy: `No heuristics. Structural invariants only.`

## CURRENT_STATE
- Validation baseline is stable on active fixtures (`repomap`, `test_1`).
- Project focus has moved to architecture refactor and LOC reduction in `canon-capture`.

## ACTIVE_REFACTOR_TARGET
- Primary file: `canon-capture/src/project/item.rs`
- Refactor model:
- Engine core (`A`)
- Rule table (`R`)
- Backend adapters/hooks (`E`)

## ACTIVE_PLAN_REFERENCE
- Plan: [PLAN.md](/workspace/ai_sandbox/canon/PLAN.md)
- State: [AGENT_STATE.md](/workspace/ai_sandbox/canon/AGENT_STATE.md)
- Plan id: `CANON_CAPTURE_LOC_REDUCTION_V1`
- Plan status: `in_progress`

## DONE_CRITERIA_STATUS
- Green baseline before refactor: `met`
- Rule-table + engine scaffold: `pending`
- `item.rs` material LOC reduction with green fixtures: `pending`

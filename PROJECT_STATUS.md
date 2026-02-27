# PROJECT_STATUS.md

## CANONICAL_HEADER
- project: `canon`
- status_epoch: `2026-02-27`
- pipeline_invariant: `Capture -> CanonIR -> Graph -> Solve -> Emit`
- policy: `If tempted to use heuristic, stop and add missing structural invariant.`

## CURRENT_STATE
- Workspace compiles.
- Canon-only pipeline remains operational.
- MIR-structured body capture is primary for fn/assoc fn in active flow.
- Residual raw body/op variants are removed from active schema/model flow.

## VALIDATED_FIXTURES
- `test_projects/test_rust_projects/capture/repomap` -> `emit/repomap` build: pass.
- `test_projects/test_rust_projects/capture/test_1` -> `emit/test_1` build: pass.

## ACTIVE_PLAN_REFERENCE
- Plan: [PLAN.md](/workspace/ai_sandbox/canon/PLAN.md)
- State: [AGENT_STATE.md](/workspace/ai_sandbox/canon/AGENT_STATE.md)
- Plan id: `CANON_BODY_STRUCTURAL_PRIMARY_V1`
- Plan status: `completed`

## DONE_CRITERIA_STATUS
- MIR-structured body ops primary: `met`
- Raw body/op variants eliminated from active path: `met`
- Validation sweep green: `met`

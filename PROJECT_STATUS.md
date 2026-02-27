# PROJECT_STATUS.md

## CANONICAL_HEADER
- project: `canon`
- status_epoch: `2026-02-27`
- pipeline_invariant: `Capture -> CanonIR -> Graph -> Solve -> Emit`
- policy: `No heuristics. Structural invariants only.`

## CURRENT_STATE
- Engine templates now own low-risk kinds plus function metadata path.
- MIR body lowering remains delegated and unchanged in structure.
- Validated fixtures remain green.

## REFACTOR_PROGRESS
- Baseline metrics:
- `item.rs`: `2134` LOC
- `canon-capture/src`: `4132` LOC
- Current:
- `item.rs`: `1995` LOC
- Completed:
- Phase 1 scaffold
- Phase 2 seam integration
- Phase 3 bootstrap set migration (`Mod`, `Struct/Union`, `Enum`, `Const`, `Static`, `TyAlias`, `Use`)
- Phase 4 slice 1 (`Fn`, `AssocFn` metadata migration)
- Pending:
- migrate `Trait`, `Impl`, `AssocTy`, `AssocConst`
- continue deletion-driven collapse in `item.rs`

## ACTIVE_PLAN_REFERENCE
- Plan: [PLAN.md](/workspace/ai_sandbox/canon/PLAN.md)
- State: [AGENT_STATE.md](/workspace/ai_sandbox/canon/AGENT_STATE.md)
- Plan id: `CANON_CAPTURE_LOC_REDUCTION_V1`
- Plan status: `in_progress`

## DONE_CRITERIA_STATUS
- Rule table active in runtime path: `met`
- Direct engine template emission (bootstrap + fn path): `met`
- Behavior parity on validated fixtures: `met`
- Material `item.rs` LOC reduction: `in_progress`

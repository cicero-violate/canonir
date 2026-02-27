# PROJECT_STATUS.md

## CANONICAL_HEADER
- project: `canon`
- status_epoch: `2026-02-27`
- pipeline_invariant: `Capture -> CanonIR -> Graph -> Solve -> Emit`
- policy: `No heuristics. Structural invariants only.`

## CURRENT_STATE
- Engine templates own active metadata lowering.
- `item.rs` has been structurally thinned by extracting metadata/type helpers into `project/helpers.rs`.
- Engine dispatch no longer carries legacy hook mode (`RuleEmit::Hook` removed).
- Phase 5 edge-template migration has started (`use_item` edge emission is rule-template driven).
- Relation projection now uses a relation-template dispatcher for parent/assoc/impl edge classes.
- Engine and relations now share `project/edge_emit.rs` for edge construction primitives.
- Body projection (`CfgEdge/Calls/ConstDep/Contains`) now also emits through shared `project/edge_emit.rs`.
- Validated fixture parity remains green.

## REFACTOR_PROGRESS
- Baseline metrics:
- `item.rs`: `2134` LOC
- `canon-capture/src`: `4132` LOC
- Current:
- `item.rs`: `1391` LOC
- `helpers.rs`: `572` LOC
- `canon-capture/src`: `4810` LOC
- Completed:
- Phase 1 scaffold
- Phase 2 seam integration
- Phase 3 metadata bootstrap migration
- Phase 4 function/assoc/trait/impl metadata migration
- Phase 6 helper extraction slice 1
- Phase 6 helper-bridge cleanup slice 2 (`collect_derives` moved, hook path removed)
- Phase 6 helper-bridge cleanup slice 3 (`item.rs` helper re-export bridge removed)
- Pending:
- Phase 5 edge-template migration (remaining DefKind edge patterns)
- Phase 7 final LOC gate with broader fixture sweep

## ACTIVE_PLAN_REFERENCE
- Plan: [PLAN.md](/workspace/ai_sandbox/canon/PLAN.md)
- State: [AGENT_STATE.md](/workspace/ai_sandbox/canon/AGENT_STATE.md)
- Plan id: `CANON_CAPTURE_LOC_REDUCTION_V1`
- Plan status: `in_progress`

## DONE_CRITERIA_STATUS
- Rule table active in runtime path: `met`
- Engine template ownership for active metadata DefKinds: `met`
- Behavior parity on validated fixtures: `met`
- Material `item.rs` LOC reduction: `met`
- Net crate LOC reduction: `pending` (requires consolidation/deletion follow-up)

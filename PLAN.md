# PLAN.md

## CANONICAL_HEADER
- plan_id: `CANON_BODY_RETURN_INVARIANTS_V1`
- scope: `Capture -> CanonIR -> Projection`
- hard_rule: `No heuristics. Structural invariants only.`
- objective: `Complete non-unit return structural reconstruction and remove projection-side todo fallback.`

## PHASE_R1_RETURN_VALUE_CAPTURE
status: `in_progress`

1. Capture MIR return-place flow (`_0`) into explicit canonical return operations.
2. Emit `CfgOp::Assign` only when RHS is structurally declared/known.
3. Preserve compile safety while expanding structural return coverage.

## PHASE_R2_RETURN_EMIT_STRICTNESS
status: `pending`

1. Remove projection-side non-unit `todo!()` injection in `emit_fn`.
2. Require body-carried return value structure for non-unit functions.
3. Fail on invariant violations rather than patching in projection.

## PHASE_R3_VALIDATION_SWEEP
status: `pending`

1. Workspace `cargo check`.
2. Fixture matrix:
   - `capture/repomap -> emit/repomap -> cargo build`
   - `capture/test_1 -> emit/test_1 -> cargo build`
3. Track remaining `todo!()` count in emitted fixtures as a structural gap metric.

## EXIT_CONDITION
status: `pending`

1. Non-unit returns are structurally represented from capture.
2. Projection no longer injects `todo!()` as a return fallback.
3. Validation sweep is green.

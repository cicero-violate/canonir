# PLAN.md

## CANONICAL_HEADER
- plan_id: `CANON_BODY_STRUCTURAL_PRIMARY_V1`
- scope: `Capture -> CanonIR -> Projection`
- hard_rule: `No heuristics. Structural invariants only.`
- objective: `Promote MIR-structured body ops to primary path and remove active Raw emission dependence.`

## PHASE_P1_STRUCTURAL_INVARIANTS
status: `completed`

1. Enforced canonical place/operand resolvability via MIR local-name resolver (params + debug info).
2. Structured ops are emitted only when local/value identity is structurally resolvable.
3. Mixed-mode raw fallback for fn/assoc fn body source removed from active path.

## PHASE_P2_CAPTURE_PRIMARY_STRUCTURED_BODY
status: `completed`

1. Fn/assoc fn capture now uses MIR structured body as primary source.
2. `Body::Raw` is no longer used in active fn/method capture flow.
3. Structured body extraction remains deterministic for method call / field access / struct literal and control-flow terminators.

## PHASE_P3_PROJECTION_RAW_SURFACE_REMOVAL
status: `completed`

1. Projection now treats `CfgOp::Raw` as invariant violation (`panic!`) rather than rendering text.
2. Structured `StructLit` rendering implemented.
3. Destination binding is declaration-safe (`let` first write, assignment on subsequent writes).

## PHASE_P4_VALIDATION_SWEEP
status: `completed`

1. Workspace `cargo check`: pass.
2. `repomap` fixture: capture -> orchestration -> emitted `cargo build`: pass.
3. `test_1` fixture: capture -> orchestration -> emitted `cargo build`: pass.

## EXIT_CONDITION
status: `completed`

1. MIR-structured body ops are primary in active capture flow.
2. Active projection path no longer accepts raw op emission.
3. Required validation matrix is green.

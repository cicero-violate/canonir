# PLAN.md

## CANONICAL_HEADER
- plan_id: `CANON_CAPTURE_LOC_REDUCTION_V1`
- scope: `canon-capture only`
- hard_rule: `No heuristics. Structural invariants only.`
- objective: `Replace large procedural lowering in item.rs with rule-table + engine architecture while preserving behavior.`

## BASELINE
status: `done`

1. Record LOC baselines: `done`
- `canon-capture/src/project/item.rs`
- `canon-capture` crate total LOC
2. Record behavior baselines: `done`
- `cargo check`
- fixture pipeline/build matrix used in current validation flow
3. Freeze invariants: `done`
- output node/edge/body shape equivalence for covered DefKinds
- no fallback/raw-path reintroduction

## PHASE_1_DOMAIN_MODEL
status: `done`

1. Add `project/rules.rs` with canonical rule schema: `done`
- `RuleSpec`
- `RulePred`
- `RuleEmit`
- `RuleEdge`
- optional narrow hook handles
2. Add `DefMeta` shape (analyzed def facts) and canonical fragment output type. `done`
3. Keep all code compile-safe with placeholders and no behavior switch yet. `done`

## PHASE_2_ENGINE_CORE
status: `done`

1. Add `project/engine.rs` with: `done`
- `analyze_def(tcx, def_id) -> DefMeta`
- `lower_def(tcx, def_id, index) -> (Vec<Node>, Vec<EdgeHint>)`
2. Implement deterministic rule match and dispatch order. `done`
3. Keep MIR body lowering delegated to existing body lowering path. `done`

## PHASE_3_RULE_BOOTSTRAP
status: `done`

1. Encode first DefKind set in rules (low-risk, high-volume boilerplate): `done`
- module `done`
- struct `done`
- enum `done`
- const `done`
- static `done`
- type alias `done`
- use `done`
- type ref/lifetime `pending` (not def-driven in current lowering flow)
2. Move repeated field extraction into shared helpers used by rules. `pending`
3. Keep legacy code path for uncovered DefKinds. `done`

## PHASE_4_FUNCTION_PATH_MIGRATION
status: `in_progress`

1. Migrate function/assoc-fn lowering metadata path into rules+engine. `done`
2. Keep body lowering call boundary unchanged (`mir_body_structural` remains isolated). `done`
3. Ensure async/unsafe/generics/where-clause wiring preserved. `done`

## PHASE_5_EDGE_TEMPLATE_MIGRATION
status: `pending`

1. Move repeated edge emission patterns into rule edge templates.
2. Keep special-case edges in explicit hooks only where structurally necessary.
3. Delete duplicated edge boilerplate from legacy branches.

## PHASE_6_SWITCHOVER_AND_DELETION
status: `pending`

1. Switch `project_item(...)` to engine-first path.
2. Remove migrated legacy branches from `item.rs`.
3. Shrink `item.rs` to orchestration + body lower integration + thin adapters.

## PHASE_7_VALIDATION_AND_LOC_GATE
status: `pending`

1. Run full compile/pipeline validation matrix.
2. Confirm structural equivalence on emitted artifacts for validated fixtures.
3. Measure LOC deltas and enforce target:
- substantial reduction in `item.rs`
- net reduction in `canon-capture` LOC

## EXECUTION_POLICY
status: `active`

1. Deliver incrementally in small compiling slices.
2. After each phase:
- compile
- run fixture matrix
- update state/status files
3. Do not add tests per current project constraint.

## EXIT_CONDITION
status: `pending`

1. `project_item` is engine/rules-driven for all active DefKinds.
2. Legacy duplicated match-spaghetti removed from `item.rs`.
3. `item.rs` reduced materially from baseline while fixtures remain green.

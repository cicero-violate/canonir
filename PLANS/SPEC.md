# SPEC: Canon Control-Flow Stabilization

DO NOT touch this file. /workspace/ai_sandbox/canon/canon-utils/canon-mini-agent/src/main.rs

## Objective
Stabilize canonical control flow in `/workspace/ai_sandbox/canon/canon-utils` so that:
- `RouteSelected -> required successor` is always correct.
- `PlanningCompleted(planned_count=0, status=missing_semantic_context) -> RouteSelected(observe) -> LoopObserved`.
- `RouteSelected(act)` only occurs when real executable work exists.
- No fake scheduler seeding, forced `Act`, manual `RequestDispatch`, or suppressed successor hacks remain.
- Duplicate observe / duplicate forwarding / duplicate fanout noise is eliminated.
- All routing decisions derive from `SemanticStateSummary`.

## Constraints
- Maintain build correctness.
- Preserve event-log successor invariants.
- Do not introduce new synthetic control events.
- Do not rely on `scheduler_len` for decision logic.
- Normalize route matching at read boundaries (`"Act"` vs `"act"`).
- Prefer narrow control-flow repairs over broad rewrites.

## Canonical Principle

```text
state -> decision -> transition
````

Where:

* `state` = `SemanticStateSummary`
* `decision` = policy / invariant evaluation
* `transition` = canonical control event emission

## Forbidden

* local mirrors deciding routes
* `scheduler_len` deciding routes
* executor patches / overrides deciding routes
* synthetic dispatch paths bypassing canonical routing
* fake queue mutations
* forced route emissions outside canonical policy / invariant flow
* duplicate observe delivery
* JSON shell contamination of raw-markdown task outputs

## Required Properties

1. `PlanningCompleted(planned_count=0, status=missing_semantic_context)` must recover via:

   * `PlanningCompleted -> RouteSelected(observe) -> LoopObserved`
2. `LoopObserved` must be emitted exactly once per observe execution.
3. `RouteSelected(act)` must occur only when real executable work exists.
4. `LoopActed` must only follow real act work.
5. Duplicate observe / duplicate forwarding / duplicate fanout noise must not exist.
6. No synthetic `RequestDispatch` path may exist.
7. Routing authority must come exclusively from `SemanticStateSummary`.
8. DECIDE and ROUTE traces must cover all decision branches and route-emission sites.
9. Executor behavior must not override planner / policy / invariant routing truth.

## Canonical Repair Targets

1. State authority migration to `SemanticStateSummary`
2. Removal of queue-driven routing (`scheduler_len`, `planned_pending`, `planned_count`)
3. Elimination of executor-level routing overrides
4. Elimination of synthetic dispatch / forced act paths
5. Exact-once observe closure
6. Full DECIDE / ROUTE trace coverage
7. Prompt / shell contract correctness at markdown vs JSON boundaries
8. Runtime freshness / event-log observability restoration

## Verification Commands

1. `cargo build -p canon-route -p canon-loop -p canon-runtime -p canon-mini-agent`
2. `cargo run --bin canon-runtime-supervisor 2>&1 | tee /tmp/runtime.trace`
3. `rg -n "planning_completed|route_selected|loop_observed|loop_acted|RequestDispatch|DECIDE TRACE|ROUTE TRACE" /tmp/runtime.trace`
4. `rg -n "scheduler_len" canon-utils/canon-route canon-utils/canon-loop canon-utils/canon-runtime canon-utils/canon-invariant`
5. `rg -n "RequestDispatch|force|override|suppress|dedup" canon-utils/canon-route/src canon-utils/canon-runtime/src`
6. `rg -n "parse_actions|json|markdown" canon-utils/canon-mini-agent/src`

## Success Criteria

* `PlanningCompleted(0, missing_semantic_context) -> RouteSelected(observe) -> LoopObserved` occurs cleanly
* `LoopObserved` is emitted exactly once per observe execution
* `loop_observed` is not duplicated by fanout / forwarding hacks
* `RouteSelected(act)` only occurs when real executable work exists
* `RouteSelected(act) -> LoopActed` occurs only for real act work
* No fake scheduler seeding remains
* No manual `RequestDispatch` or synthetic forced `Act` remains
* All routing decisions derive exclusively from `SemanticStateSummary`
* Build passes for touched crates
* Trace shows canonical control succession without deadlocks or duplicate control spam

## Agent Ownership Model

* `SPEC.md` is canonical truth
* planner derives executor lane plans from `SPEC.md`
* executors execute lane plans and report evidence only
* verifier judges code against `SPEC.md`
* diagnostics ranks failures against `SPEC.md`


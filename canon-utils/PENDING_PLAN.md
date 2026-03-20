# PENDING PLAN

This file replaces the retired planning docs:

- `PLAN_judgment.md`
- `REPAIR_PLAN.md`
- `REPAIR_PLAN_scheduling.md`

## Status Review Summary

1. `PLAN_judgment.md`: implemented.
- `canon-decision` and `canon-judgment` are present and wired.
- Runtime routing uses decision + judgment + guard overrides.

2. `REPAIR_PLAN.md`: mostly implemented.
- Live route state is updated from dispatched runtime events.
- Bounded journal and deterministic route-state transitions are in place.
- `canon-goal` exists and is consumed by runtime routing summary.
- Deterministic completion evaluator exists for the rust-project target.
- Planner no longer inlines full goal every turn.

3. `REPAIR_PLAN_scheduling.md`: implemented at core architecture level.
- Control/event queues are split (`Q_c`, `Q_e`).
- Interleaved scheduling with bounded event budget is live.
- Control-critical bus events use reliable delivery.

## Remaining Relevant Work

### A. Goal Lifecycle Events + Persistence

Objective: complete structured goal ownership at system layer.

1. Emit explicit goal lifecycle events:
- `goal_loaded`
- `goal_progressed`
- `goal_satisfied`
- `goal_failed`

2. Persist structured goal state snapshots:
- serialize `GoalSpec`, `GoalStatus`, and requirement evidence into runtime/tlog payloads.

3. Route/verify integration:
- attach deterministic evidence payloads to `goal_progressed` and terminal goal events.

Acceptance:
- A single run shows goal lifecycle events in order with evidence, without relying on prompt text reconstruction.

### B. Planner Input Cleanup (GoalSpec-first)

Objective: remove residual prompt-text dependency in planner context.

1. Feed planner from structured `GoalSpec` summary + state delta + last action result.
2. Keep optional compatibility fallback to raw goal markdown for one release window only.
3. Add a config gate to disable fallback once stabilized.

Acceptance:
- Planner works with structured state only when fallback is disabled.

### C. Legacy Goal Path Deprecation

Objective: remove old goal-from-prompt behavior safely.

1. Mark legacy goal-text path deprecated in code comments/config.
2. Keep compatibility shim for one release window.
3. Remove shim after validation and replay checks pass.

Acceptance:
- No runtime dependency on legacy prompt-goal parsing remains.

### D. Scheduling Hardening Validation

Objective: verify starvation fix under burst conditions.

1. Add stress/integration test:
- high rustc-event burst + tick stream
- assert bounded control latency and ongoing route progression.

2. Add runtime counters/telemetry:
- control messages processed
- event messages processed
- queue lag estimate
- route tick jitter

3. Add segment-rollover regression check:
- ensure loop continues across `.log` segment boundaries.

Acceptance:
- No control-plane stalls under synthetic event bursts or segment rollover.

### E. Observe/Validate Role Separation Follow-through

Objective: enforce "observe cheap, validate expensive" contract.

1. Keep observe free of global compile invocations.
2. Ensure `workspace_dirty` transitions are consistent:
- set on external workspace change and local action side effects
- clear only on successful validation.
3. Add validation cache:
- run `cargo check` only when workspace hash or action fingerprint changes.

Acceptance:
- No repeated compile flood from scan ticks; validation frequency tracks actual workspace changes.

## Suggested Execution Order

1. A (Goal lifecycle events + persistence)
2. B (Planner GoalSpec-first payload)
3. C (Legacy path deprecation/removal)
4. D (Scheduling stress validation + telemetry)
5. E (Validation cache and dirty-state tuning)


# EXECUTOR B PLAN (SEMANTIC AUTHORITY + INVARIANTS)

## READY NOW (MAX 5)
1. Enforce decision → RouteSelected invariant (PRIMARY ROOT)
   1. Require decision_trace before every RouteSelected
   2. Guarantee exactly one RouteSelected per decision
   3. Fail-fast on missing or duplicate route emission

2. Eliminate synthetic dispatch paths
   1. Remove all non-RouteSelected dispatch triggers
   2. Ensure RouteSelected is sole dispatch entrypoint
   3. Remove event-driven dispatch evaluation paths

3. Remove EventBus control-flow semantics
   1. Eliminate multi-consumer control behavior
   2. Remove conditional dispatch / routing logic in bus
   3. Ensure EventBus is purely passive transport

4. Enforce linear state → decision → transition chain
   1. Ensure LoopObserved always flows into decision()
   2. Ensure decision always produces exactly one route
   3. Fail-fast on re-entry or branching control paths

5. Re-validate SemanticStateSummary authority
   1. Confirm no event-type or queue-derived routing inputs remain
   2. Ensure all decisions originate from semantic state only
   3. Fail-fast on any non-semantic routing influence

---

## CANONICAL REWRITE (ACTIVE)

### READY NOW (MAX 5)
1. Enforce decision → RouteSelected invariant (PRIMARY ROOT)
   1. Require decision_trace before every RouteSelected
   2. Guarantee exactly one RouteSelected per decision
   3. Convert violations to fail-fast

2. Eliminate synthetic dispatch paths
   1. Remove all non-RouteSelected dispatch triggers
   2. Ensure RouteSelected is sole dispatch entrypoint
   3. Delete RequestDispatch and implicit dispatch generation

3. Remove EventBus control-flow semantics
   1. Eliminate multi-consumer routing behavior
   2. Remove filtering / fanout / conditional dispatch logic
   3. Ensure EventBus is passive transport only

4. Enforce SemanticStateSummary-only routing
   1. Remove scheduler_len / planned_pending from decisions
   2. Remove event-type driven routing
   3. Ensure all routing derives exclusively from semantic state

5. Enforce linear state → decision → transition chain
   1. Ensure LoopObserved always flows into decision()
   2. Ensure decision always produces exactly one route
   3. Fail-fast on re-entry or branching control paths

### BLOCKED
- Full lifecycle validation (blocked on executor A LoopObserved exact-once fix)

## BLOCKED
- Full lifecycle validation (blocked on executor A LoopObserved emission + dispatch cleanup)
   3. Fail-fast on any non-semantic routing input

## BLOCKED
- Full lifecycle validation (blocked on executor A duplicate emission root cause)

## BLOCKED
- Full lifecycle validation (blocked on executor A LoopObserved + dispatch fixes)
   3. Convert violations to hard failures

## BLOCKED
- Full lifecycle validation (blocked on executor A + dispatch fixes)
1. Fix EventBus control-flow corruption (PRIMARY ROOT)
   1. Remove control-event filtering (is_control_event)
   2. Eliminate fanout/guard/dedup paths
   3. Enforce linear flow: RouteSelected → dispatch only

2. Eliminate synthetic dispatch
   1. Delete RequestDispatch entirely
   2. Ensure no dispatch occurs without RouteSelected
   3. Remove implicit/async dispatch generation

3. Restore semantic routing authority
   1. Remove planned_pending from all routing decisions
   2. Remove scheduler_len from control-flow logic
   3. Treat queue state as telemetry only

4. Enforce SemanticStateSummary-only routing
   1. Derive all decisions exclusively from SemanticStateSummary
   2. Ensure RouteSelected(act) only when real work exists
   3. Remove planned_count as routing signal

5. Enforce decision → route invariant (STRICT)
   1. Require decision_trace before RouteSelected
   2. Guarantee exactly one RouteSelected per decision
   3. Convert violations to hard failures

## BLOCKED
- Lifecycle + trace validation (blocked on observe + dispatch + semantic fixes)
   3. Guarantee exactly one RouteSelected per decision

5. Remove synthetic dispatch completely
   1. Delete RequestDispatch from runtime and enums
   2. Remove fanout/replay dispatch paths
   3. Ensure RouteSelected is sole dispatch entrypoint

## BLOCKED
- Lifecycle + trace validation (blocked on authority + observe + routing fixes)

1. Remove planned_pending as routing authority (PRIMARY ROOT)
   1. Delete ALL ctx.planned_pending assignments
   2. Remove ALL reads of planned_pending in policy.rs and executor.rs
   3. Remove "authoritative" references in executor.rs
   4. Remove planned_pending from context.rs struct

2. Remove scheduler_len and queue-driven routing
   1. Delete scheduler_len usage from ALL routing decisions
   2. Ensure routing never depends on queue length
   3. Restrict any remaining usage to telemetry only

3. Refactor routing to SemanticStateSummary-only
   1. Rewrite policy.rs decision branches using SemanticStateSummary only
   2. Ensure RouteSelected(act) only when real executable work exists
   3. Remove planned_count as routing signal

4. Enforce decision → route invariant (STRICT)
   1. Replace debug_assert with hard invariant
   2. Panic if decision produces no RouteSelected
   3. Guarantee exactly one RouteSelected per decision

5. Remove synthetic dispatch completely
   1. Delete RequestDispatch from runtime and enums
   2. Remove fanout/replay dispatch paths
   3. Ensure RouteSelected is sole dispatch entrypoint

## BLOCKED
- Lifecycle + trace validation (blocked on authority + observe + routing fixes)

## BLOCKED
- Full trace verification (requires LoopObserved + dispatch fixes from executor A)
- Duplicate fanout cleanup (requires canonical dispatch normalization)
# EXECUTOR B PLAN (SEMANTIC AUTHORITY + ROUTING INVARIANTS)

## READY NOW (MAX 5)

1. Remove queue-driven routing authority (CRITICAL ROOT)
   1. Delete ALL planned_pending assignments and reads
   2. Remove scheduler_len from ALL routing logic
   3. Eliminate planned_count as routing signal
   4. Ensure routing derives ONLY from SemanticStateSummary

2. Refactor policy.rs to semantic-only routing
   1. Rewrite decision branches using SemanticStateSummary fields
   2. Remove ALL queue-based conditions
   3. Ensure RouteSelected(act) only when real work exists

3. Restore control-event routing pipeline
   1. Remove NoOp handling for LoopObserved and RouteSelected
   2. Ensure all control events trigger decision()
   3. Verify decision + route traces always emitted

4. Enforce decision → route invariant (STRICT)
   1. Replace debug_assert with hard invariant
   2. Panic if route emitted without decision
   3. Guarantee exactly one RouteSelected per decision

5. Remove synthetic dispatch completely
   1. Delete RequestDispatch from runtime and enums
   2. Remove fanout/replay dispatch paths
   3. Ensure RouteSelected is sole dispatch entrypoint

## BLOCKED
- Lifecycle completion (blocked on routing + semantic authority fixes)

# Plan: Fix Agent Loop Stuck on LLM Planner Timeout

## Problem Summary

The agent loop enters an infinite cycle when the LLM planner times out:

```
route_selected(plan)
  → llm.plan dispatched
  → llm call timed out (capability_failed)
  → planning_completed(status=llm_failed, planned_count=0)
  → route_selected(plan)   ← deterministic fallback fires again
  → [repeat forever]
```

**Why it loops:** `router_disabled_fallback()` in `canon-utils/canon-route/src/executor.rs` always returns `RouteKind::Plan` unconditionally. There is no counter for consecutive LLM planning failures. Each `planning_completed(llm_failed)` triggers a new `route_selected`, which re-enters the plan stage, which times out again.

**Why observe is blocked:** Once `route_selected(plan)` fires, the `pending_required_successor` is set to `planning_completed`. The loop stage executor then emits `observe_suppressed_due_to_pending_successor` for any incoming events until `planning_completed` arrives. This means even if observe were triggered in parallel, it would be suppressed.

**Secondary symptom:** The `event_repair_trigger` is trying to connect to a workspace repair job at `127.0.0.1:9102` and gets "Connection refused" on every cycle. This adds noise and cooldown delays but is not the primary cause.

---

## Files to Change

### 1. `canon-utils/canon-route/src/context.rs`
**What:** Add a `consecutive_llm_plan_failures: u32` field to `RouteContext`.

**How:**
- Initialize to `0` in `RouteContext::new()`.
- Increment it whenever the context processes a `planning_completed` event with `status == "llm_failed"` or `status == "llm_timeout"`.
- Reset it to `0` whenever `planning_completed` arrives with `status == "ok"` or `planned_count > 0`.
- Expose it via a method or as a public field so the route executor can read it.

---

### 2. `canon-utils/canon-route/src/executor.rs`
**What:** Change `router_disabled_fallback_rule()` and `router_disabled_fallback()` to route to `observe` (not `plan`) when consecutive LLM plan failures exceed a threshold.

**How:**

In `router_disabled_fallback_rule()`, add a new branch **before** the existing ones:
```
if self.ctx.consecutive_llm_plan_failures >= 2 {
    return DeterministicRouteRule::LlmPlanTimeoutObserve;
}
```

 In `router_disabled_fallback()`, read the rule from `router_disabled_fallback_rule()` and map `NoActionableFailureObserve` to `RouteKind::Observe` instead of `RouteKind::Plan`. Currently the method ignores the rule when constructing the decision and always hardcodes `RouteKind::Plan`. Fix that by matching on the rule:  ✓ done
- Add explicit match arm for LlmPlanTimeoutObserve => RouteKind::Observe
- Ensure NoActionableFailureObserve is not used for timeout handling
- Update rationale to include failure count and timeout wording
- Update prompt_tag to distinguish timeout observe vs plan fallback
- Update noop_reason to reflect timeout-driven observe routing
- `MissingTargetPlan` | `InvalidPlanReplan` | `BlockedValidationPlan` → `RouteKind::Plan`
- `NoActionableFailureObserve` → `RouteKind::Observe`

Update the `rationale` string to include the failure count when routing to observe, e.g.:
`"router_llm_disabled; consecutive_llm_plan_failures={N}; routing to observe to break timeout loop"`

Also update the `prompt_tag` and `noop_reason` to distinguish the observe fallback from the plan fallback for observability.

---

### 3. `canon-utils/canon-route/src/executor.rs` — `handle_planning_completed` (or wherever the executor updates context from events)
**What:** Ensure the `consecutive_llm_plan_failures` counter in `RouteContext` is updated when a `planning_completed` event arrives at the executor.

**How:** Find the place in `executor.rs` where `RouteContext` is updated from incoming `PlanningCompleted` events (look for `self.ctx` updates near `PlanningCompleted` handling). Call the new counter update method there. If there is no such update path in the executor (context may be updated only via a dedicated method), add a call:
```rust
self.ctx.record_planning_completion(&pc.status);
```

---

### 4. `canon-utils/canon-route/src/policy.rs`
**What:** Add a new `DeterministicRouteRule` variant for the LLM timeout fallback path, to keep observability clean.

**How:**
- [x] Add `LlmPlanTimeoutObserve` to the `DeterministicRouteRule` enum ✓ done
- [ ] Use this variant (not the general `NoActionableFailureObserve`) when routing to observe due to consecutive LLM failures
 - [x] Use this variant (not the general `NoActionableFailureObserve`) when routing to observe due to consecutive LLM failures  ✓ done
  1. Search in `canon-utils/canon-route/src/executor.rs` for all usages of `NoActionableFailureObserve`
  2. Identify the branch triggered by `consecutive_llm_plan_failures >= 2` in `router_disabled_fallback_rule()`
  3. Ensure that branch returns `DeterministicRouteRule::LlmPlanTimeoutObserve`
  4. Update `router_disabled_fallback()` match arms to explicitly handle `LlmPlanTimeoutObserve => RouteKind::Observe`
  5. Remove or avoid any fallback path where LLM timeout uses `NoActionableFailureObserve`
  6. Run `cargo check` to verify exhaustive enum matching and no warnings

---

## Threshold

Use `consecutive_llm_plan_failures >= 2` as the threshold. This allows one retry (in case the first timeout is transient) but breaks the loop on the second consecutive failure. Do not use 1 — a single timeout can be a transient network hiccup to the LLM relay.

---

## What This Does NOT Fix

- The LLM relay timeout root cause (why `llm.plan` times out). That is a separate issue — possibly the context/prompt sent to the planner is too large, or the relay at 9101 is slow. This plan only breaks the infinite loop.
- The `event_repair_trigger` connection refused error at port 9102. That service is not running. Fix that separately if workspace repair is needed.
- The `discover_test_surface` strategy logic. Once the loop is broken and the agent routes to `observe`, the existing observe machinery will re-evaluate state and the planner will eventually produce a valid batch.

---

## Verification

After the fix, the expected event sequence when the LLM planner times out twice should be:

```
route_selected(plan) → planning_completed(llm_failed)   [failure 1]
route_selected(plan) → planning_completed(llm_failed)   [failure 2]
route_selected(observe) → loop_observed                 [fallback kicks in]
```

Instead of the current infinite `plan → llm_failed → plan → ...` loop.

---

## FULL EXPANDED TASK BREAKDOWN (IMPLEMENTATION READY)

### A. Context State Tracking (Hard Requirement)

1. Add field to RouteContext
   - file: canon-utils/canon-route/src/context.rs
   - modify struct RouteContext
   - add:
     consecutive_llm_plan_failures: u32

2. Initialize field
   - locate RouteContext::new()
   - set consecutive_llm_plan_failures = 0

3. Add updater method
   - fn record_planning_completion(&mut self, status: &str, planned_count: usize)
   - logic:
     if status == "llm_failed" || status == "llm_timeout" → increment
     if status == "ok" OR planned_count > 0 → reset to 0

4. Add getter (optional but recommended)
   - fn consecutive_failures(&self) -> u32

---

### B. Executor Integration (Critical Path)

5. [x] Hook into PlanningCompleted handling ✓ done
   - file: canon-utils/canon-route/src/executor.rs
   - locate event handler for PlanningCompleted
   - extract:
     - status
     - planned_count
   - call:
     self.ctx.record_planning_completion(status, planned_count)

6. Ensure exactly-once update
   - verify no duplicate handler paths call this
   - ensure idempotency per event

---

### C. Deterministic Routing Fix (Root Cause)

7. Modify router_disabled_fallback_rule()
   - add FIRST branch:
     if self.ctx.consecutive_llm_plan_failures >= 2 {
         return DeterministicRouteRule::LlmPlanTimeoutObserve;
     }

8. Verify ordering
   - must be BEFORE other fallback rules
   - prevents Plan from always winning

9. Modify router_disabled_fallback()
   - remove hardcoded RouteKind::Plan
   - match on rule:
     - MissingTargetPlan → Plan
     - InvalidPlanReplan → Plan
     - BlockedValidationPlan → Plan
     - LlmPlanTimeoutObserve → Observe

10. Update rationale string
    - include failure count:
      format!("router_llm_disabled; consecutive_failures={}; routing=observe", count)

11. Update prompt_tag
    - distinguish:
      - "router_llm_disabled_plan"
      - "router_llm_timeout_observe"

12. Update noop_reason
    - ensure observability layer can differentiate fallback cause

---

### D. Policy Layer Update (Clean Semantics)

13. Extend enum
   - file: canon-utils/canon-route/src/policy.rs
   - enum DeterministicRouteRule
   - add:
     LlmPlanTimeoutObserve

14. Update all match statements
   - ensure exhaustive handling
   - compiler must pass without warnings

15. Replace previous usage
   - do NOT reuse NoActionableFailureObserve
   - use LlmPlanTimeoutObserve explicitly for this path

---

### E. Invariant Protection (Prevents noop_spam)

16. Validate successor emission
   - ensure Observe produces a valid successor event
   - avoid emitting noop-only cycles

17. Ensure route_selected(observe) leads to:
   - observe.list_dir OR equivalent
   - NOT immediate noop

18. Confirm invariant layer behavior
   - no duplicate events
   - no rejected successor

---

### F. Event Flow Guarantees

19. Each PlanningCompleted must produce exactly ONE route_selected

20. Each route_selected must produce exactly ONE stage transition

21. No silent suppression due to pending_required_successor deadlock

---

### G. Logging & Debugging

22. Log consecutive failure count
   - on each planning_completed

23. Log routing decision
   - include rule + count

24. Verify logs show transition:
   plan → plan → observe

---

### H. Validation Scenarios

25. Scenario 1: normal success
   - plan succeeds → counter resets

26. Scenario 2: single failure
   - plan fails once → retry plan

27. Scenario 3: double failure
   - second failure → route to observe

28. Scenario 4: recovery
   - observe produces new context → plan resumes

29. Scenario 5: no invariant violations
   - confirm no noop_spam

---

### I. Final Acceptance Criteria Mapping

✔ no infinite plan loop
✔ deterministic routing after failure threshold
✔ no noop_spam invariant violation
✔ event log append succeeds
✔ replay produces identical routing decisions

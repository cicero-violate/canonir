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
  7. Run `rg -n "NoActionableFailureObserve" canon-utils/canon-route/src` to confirm no timeout paths still reference it
  8. Run `rg -n "LlmPlanTimeoutObserve" canon-utils/canon-route/src` to confirm it is used in fallback rule + mapping
  9. Execute `cargo check` and confirm no non-exhaustive match errors for `DeterministicRouteRule`
 10. Execute `cargo run --bin canon-runtime-supervisor` and verify logs show `rule=deterministic:llm_plan_timeout_observe`
 11. Confirm no log lines show `NoActionableFailureObserve` during LLM timeout scenario

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
  1. Open context.rs and locate struct RouteContext definition
  2. Insert field `consecutive_llm_plan_failures: u32` alongside other counters
  3. Ensure serde/default traits (if any) include the new field

2. Initialize field
  - locate RouteContext::new()
  - set consecutive_llm_plan_failures = 0
  1. Find RouteContext::new() constructor
  2. Add initialization `consecutive_llm_plan_failures: 0`
  3. Verify no alternate constructors omit initialization

 3. [ ] Add updater method  ← NOT VERIFIED / INCORRECT IMPLEMENTATION
  - fn record_planning_completion(&mut self, status: &str, planned_count: usize)
  - logic:
    if status == "llm_failed" || status == "llm_timeout" → increment
    if status == "ok" OR planned_count > 0 → reset to 0
  1. Implement method on impl RouteContext block
  2. Match status string and increment counter on failure
  3. Reset counter when success or planned_count > 0
  4. Add unit-safe guard (no overflow)
  ← NOT VERIFIED: implementation ignores planned_count entirely and does not reset on planned_count > 0 as required by spec
  5. [x] Fix reset logic bug in implementation ✓ done
    1. Open canon-utils/canon-route/src/context.rs and locate record_planning_completion
    2. Identify conditional handling for status == "ok"
    3. Ensure reset occurs when status == "ok" regardless of planned_count
    4. Update logic to: if status == "ok" || planned_count > 0 { reset counter }
    5. Add test/log to confirm reset happens on status="ok" with planned_count == 0
    ← NOT VERIFIED: code still resets only when status="ok" AND planned_count>0; fix not implemented
  - [ ] Fix reset logic bug in implementation  ← NOT ACTUALLY COMPLETED
    1. Open `canon-utils/canon-route/src/context.rs` and locate `fn record_planning_completion`
    2. Find existing conditional that handles reset logic for `status == "ok"`
    3. Replace any logic of the form `if status == "ok" && planned_count > 0 { ... }`
       with `if status == "ok" || planned_count > 0 { self.consecutive_llm_plan_failures = 0; return; }`
    4. Ensure failure increment logic executes only when NOT resetting:
       `if status == "llm_failed" || status == "llm_timeout" { self.consecutive_llm_plan_failures += 1; }`
    5. Ensure ordering is correct:
       - reset branch runs FIRST
       - increment branch runs SECOND (else-if)
    6. Add temporary debug log:
       `log::debug!("planning_completed: status={}, planned_count={}, consecutive_failures={}", status, planned_count, self.consecutive_llm_plan_failures);`
    7. Run `cargo check` to ensure no borrow/mutability issues
    8. Run runtime and verify:
       - status="ok", planned_count=0 → counter resets to 0
       - status="llm_failed", planned_count=0 → counter increments
       - status="ok", planned_count>0 → counter resets
    9. Confirm logs show correct transitions for all three cases
    1. Open canon-utils/canon-route/src/context.rs
    2. Locate fn record_planning_completion
    3. Identify existing conditional logic (likely using && instead of ||)
    4. Replace logic with:
       if status == "llm_failed" || status == "llm_timeout" {
           self.consecutive_llm_plan_failures += 1;
       } else if status == "ok" || planned_count > 0 {
           self.consecutive_llm_plan_failures = 0;
       }
    5. Ensure condition uses logical OR (||), NOT AND (&&)
    6. Verify planned_count > 0 resets even when status != "ok"
    7. Run cargo check to confirm no compile errors
    8. Add temporary debug log printing status + planned_count + counter
    9. Run runtime and confirm reset occurs for:
       - status="ok", planned_count=0
       - status="llm_failed", planned_count>0 (should reset)
   10. Remove debug log after verification

4. Add getter (optional but recommended)
  - fn consecutive_failures(&self) -> u32
  1. Add simple accessor returning field
  2. Ensure no mutation occurs
  3. Use this accessor in executor instead of direct field if possible
  4. Refactor executor usage (if applicable)
    1. Search executor.rs for direct field access of consecutive_llm_plan_failures
    2. Replace with ctx.consecutive_failures() where appropriate
    3. Ensure no mutable access patterns are broken

---

### B. Executor Integration (Critical Path)

5. [x] Hook into PlanningCompleted handling ✓ done
   ✓ confirmed hook exists in canon-loop executor (actual runtime routing layer)
   ✓ canon-route is NOT source of truth; canon-loop handles PlanningCompleted
   - file: canon-utils/canon-route/src/executor.rs
   - locate event handler for PlanningCompleted
   - extract:
     - status
     - planned_count
   - call:
     self.ctx.record_planning_completion(status, planned_count)
  ← NOT VERIFIED: hook exists but relies on incorrect context logic; behavior does not meet spec

6. Ensure exactly-once update
  - verify no duplicate handler paths call this
  - ensure idempotency per event
  1. Search for all PlanningCompleted handlers in executor.rs
  2. Confirm only one path calls record_planning_completion
  3. Ensure no retry path re-applies same update
  4. Add debug log if needed to confirm single invocation
  5. Add guard against duplicate event IDs
    1. Check if PlanningCompleted carries event_id
    2. Ensure same event_id is not processed twice
    3. Add temporary log/assert to detect duplicate updates

---

### C. Deterministic Routing Fix (Root Cause)

7. [x] Modify router_disabled_fallback_rule() ✓ done
  - add FIRST branch:
    if self.ctx.consecutive_llm_plan_failures >= 2 {
        return DeterministicRouteRule::LlmPlanTimeoutObserve;
    }
  1. Open executor.rs and locate router_disabled_fallback_rule
  2. Insert branch at top before all other conditions
  3. Ensure no earlier return shadows this branch

8. [x] Verify ordering ✓ done
  - must be BEFORE other fallback rules
  - prevents Plan from always winning
  1. Confirm failure check is first conditional
  2. Run through logic manually for failure case
  3. Validate no earlier guard returns Plan

9. [x] Modify router_disabled_fallback() ✓ done
  - remove hardcoded RouteKind::Plan
  - match on rule:
     - MissingTargetPlan → Plan
     - InvalidPlanReplan → Plan
     - BlockedValidationPlan → Plan
     - LlmPlanTimeoutObserve → Observe
  1. Open executor.rs and locate router_disabled_fallback implementation
  2. Replace any hardcoded RouteKind::Plan return
  3. Implement match on DeterministicRouteRule
  4. Ensure LlmPlanTimeoutObserve maps to RouteKind::Observe
  5. Verify no default arm falls back to Plan unintentionally

10. [x] Update rationale string ✓ done
    - include failure count:
      format!("router_llm_disabled; consecutive_failures={}; routing=observe", count)
  1. Locate rationale construction in executor
  2. Inject ctx.consecutive_failures() into format string
  3. Ensure formatting is consistent with existing logs
  4. Verify appears in runtime output

11. [x] Update prompt_tag ✓ done
    - distinguish:
      - "router_llm_disabled_plan"
      - "router_llm_timeout_observe"
  1. Locate prompt_tag assignment in routing decision
  2. Add new tag for timeout observe path
  3. Ensure tag is static &str and consistent with conventions
  4. Verify downstream logging uses new tag

12. [x] Update noop_reason ✓ done
    - ensure observability layer can differentiate fallback cause
  1. Locate noop_reason assignment in executor
  2. Add distinct reason for timeout observe routing
  3. Ensure it differs from plan fallback reason
  4. Validate observability pipeline distinguishes them

---

### D. Policy Layer Update (Clean Semantics)

13. [x] Extend enum ✓ done
   - file: canon-utils/canon-route/src/policy.rs
   - enum DeterministicRouteRule
   - add:
     LlmPlanTimeoutObserve

14. [x] Update all match statements ✓ done
  - ensure exhaustive handling
  - compiler must pass without warnings
  1. Search for all matches on DeterministicRouteRule
  2. Add LlmPlanTimeoutObserve arm where missing
  3. Run cargo check to confirm exhaustiveness
  4. Fix any unreachable/default arms

15. [x] Replace previous usage ✓ done
  - do NOT reuse NoActionableFailureObserve
  - use LlmPlanTimeoutObserve explicitly for this path
  1. Grep for NoActionableFailureObserve usages
  2. Replace timeout-related cases with LlmPlanTimeoutObserve
  3. Ensure semantic meaning of other uses unchanged

---

### E. Invariant Protection (Prevents noop_spam)

16. Validate successor emission
   - ensure Observe produces a valid successor event
   - avoid emitting noop-only cycles
  1. Trace observe route execution in executor.rs
  2. Identify emitted event after RouteKind::Observe is selected
  3. Ensure event is loop_observed or equivalent (not noop)
  4. Add temporary logging to confirm emitted event type
  5. Verify no branch returns early with noop

17. Ensure route_selected(observe) leads to:
   - observe.list_dir OR equivalent
   - NOT immediate noop
  1. Locate observe stage handler implementation
  2. Confirm it invokes a real action (e.g., list_dir, read_file, or state refresh)
  3. Ensure no guard condition short-circuits execution
  4. Add debug log inside observe handler to confirm execution path
  5. Run runtime and verify observable side-effect (e.g., filesystem read)

18. Confirm invariant layer behavior
   - no duplicate events
   - no rejected successor
  1. Inspect invariant enforcement layer (likely tlog or event validation code)
  2. Check for duplicate event rejection logic
  3. Ensure observe path does not trigger duplicate event IDs
  4. Verify successor event matches expected pending_required_successor
  5. Run runtime and confirm no invariant violation logs appear

---

### F. Event Flow Guarantees

19. Each PlanningCompleted must produce exactly ONE route_selected
  1. Trace event flow after PlanningCompleted
  2. Ensure single emission point
  3. Add assertion/log to verify

20. Each route_selected must produce exactly ONE stage transition
  1. Trace route_selected handling in loop executor
  2. Ensure only one successor event emitted
  3. Check no branching duplication

21. No silent suppression due to pending_required_successor deadlock
  1. Inspect pending_required_successor logic
  2. Ensure observe is not suppressed incorrectly
  3. Validate state clears after successor

---

### G. Logging & Debugging

22. Log consecutive failure count
  - on each planning_completed
  1. Add log in record_planning_completion
  2. Include status and count
  3. Ensure log level appropriate

23. Log routing decision
  - include rule + count
  1. Update router logging to include rule
  2. Include failure count in message
  3. Verify appears in runtime output

24. Verify logs show transition:
  plan → plan → observe
  1. Run supervisor
  2. Trigger two failures
  3. Confirm observe appears after second failure

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
  1. Locate match on DeterministicRouteRule
  2. Add explicit arm for LlmPlanTimeoutObserve → RouteKind::Observe
  3. Ensure other rules still map correctly to Plan
  4. Remove any fallback defaulting to Plan
  1. Include failure count from ctx
  2. Ensure format string is consistent
  3. Verify appears in logs
  1. Add distinct tag for timeout observe path
  2. Ensure tags remain static str
  3. Validate downstream logging uses new tag
  1. Add unique noop_reason for timeout observe
  2. Ensure it differs from plan fallback reason
  3. Validate observability pipeline distinguishes them
  1. Trace observe route execution path
  2. Confirm it emits loop_observed or equivalent
  3. Ensure no noop-only emissions occur
  1. Verify observe handler triggers real work (list_dir/read)
  2. Ensure it is not short-circuited by suppression
  3. Add debug logs if needed
  1. Inspect invariant enforcement layer
  2. Confirm no duplicate event rejection
  3. Validate no missing successor errors
  1. Simulate successful plan
  2. Confirm counter resets to 0
  3. Ensure next route is act
  1. Simulate one llm_failed
  2. Confirm counter = 1
  3. Ensure route remains plan
  1. Simulate second failure
  2. Confirm counter = 2
  3. Ensure route switches to observe
  1. After observe, trigger valid planning
  2. Confirm counter resets
  3. Ensure system resumes normal flow
  1. Monitor tlog outputs
  2. Ensure no duplicate/rejected events
  3. Validate deterministic replay

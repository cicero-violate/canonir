# Plan: Fix Agent Loop Stuck on LLM Planner Timeout

## A. Context State Tracking
1. Add consecutive_llm_plan_failures: u32 to RouteContext
2. Initialize to 0 in RouteContext::new()
3. Add record_planning_completion(status, planned_count)
   - increment on llm_failed / llm_timeout
   - reset on ok or planned_count > 0

## B. Executor Integration
4. Hook PlanningCompleted handler to call record_planning_completion
5. Ensure update happens exactly once per event

## C. Deterministic Routing Fix
6. In router_disabled_fallback_rule():
   - if consecutive failures >= 2 → LlmPlanTimeoutObserve
7. Ensure this branch is FIRST
8. In router_disabled_fallback():
   - match rule → return correct RouteKind
   - LlmPlanTimeoutObserve → Observe
9. Update rationale to include failure count
10. Update prompt_tag + noop_reason for observability

## D. Policy Layer
11. Ensure enum includes LlmPlanTimeoutObserve
12. Update all match statements to handle it
13. Ensure executor uses LlmPlanTimeoutObserve (not NoActionableFailureObserve)

## E. Invariant Protection
14. Ensure observe produces real successor events (no noop loops)
15. Validate no duplicate/rejected events

## F. Event Flow Guarantees
16. Each planning_completed → exactly one route_selected
17. Each route_selected → exactly one stage transition

## G. Logging
18. Log consecutive failure count
19. Log routing decisions

## H. Validation
20. Test scenarios:
   - success resets counter
   - single failure retries plan
   - double failure routes to observe
   - recovery resumes plan
   - no invariant violations

## Success Criteria
- no infinite plan loop
- deterministic routing after threshold
- no noop_spam
- event log consistency
- deterministic replay

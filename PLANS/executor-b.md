# EXECUTOR B PLAN (SEMANTIC AUTHORITY + INVARIANTS)

## READY NOW (MAX 5)

1. Enforce decision→route invariant (CORE CONTROL BREAK)
   1. Open canon-route/src/executor.rs
   2. Identify decision paths without RouteSelected
   3. Replace debug_assert/no-op with hard invariant
   4. Guarantee exactly one RouteSelected per decision

2. Enforce SemanticStateSummary-only routing (PRIMARY AUTHORITY FIX)
   1. Open canon-route/src/policy.rs
   2. Remove scheduler_len / planned_pending / planned_count
   3. Replace ALL routing logic with SemanticStateSummary predicates ONLY
   4. Add invariant: routing derives exclusively from semantic state

3. Remove queue-driven routing inputs globally (ROOT CAUSE CLEANUP)
   1. Audit canon-mini-agent + invariant + context layers
   2. Eliminate scheduler_len from ALL decision logic
   3. Restrict remaining usage to telemetry only

4. Normalize route matching (ACT vs act correctness)
   1. Audit route parsing boundaries
   2. Normalize casing at read boundaries
   3. Ensure no mismatched route branches exist

5. Validate semantic recovery path (SPEC REQUIREMENT)
   1. Ensure PlanningCompleted(0, missing_semantic_context)
   2. Produces RouteSelected(observe)
   3. Followed by exactly one LoopObserved

## BLOCKED
- Full trace verification (requires LoopObserved + dispatch fixes from executor A)
- Duplicate fanout cleanup (requires canonical dispatch normalization)

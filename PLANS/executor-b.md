# EXECUTOR B PLAN (SEMANTIC AUTHORITY + INVARIANTS)

## READY NOW (MAX 5)

1. Enforce semantic-state-only routing
   1. Open canon-route/src/policy.rs
   2. Run rg -n "scheduler_len|planned_pending|planned_count" canon-utils
   3. Identify all queue-driven decision branches
   4. Replace with SemanticStateSummary predicates only

2. Remove scheduler_len from plan.rs
   1. Open canon-mini-agent/src/plan.rs
   2. Locate scheduler_len usage
   3. Remove all decision logic using scheduler_len
   4. Replace with semantic-state derived conditions

3. Remove executor-level routing decisions
   1. Open canon-route/src/executor.rs
   2. Identify all branching affecting routing
   3. Remove overrides and local decisions
   4. Ensure executor only executes policy output

4. Restore DECIDE + ROUTE trace coverage
   1. Ensure every decision emits DECIDE (canon-invariant)
   2. Ensure every RouteSelected emits ROUTE
   3. Audit early returns for missing traces

5. Enforce LoopObserved invariant
   1. Open canon-loop/src/stage/observe.rs
   2. Enumerate all exit paths
   3. Guarantee exactly one LoopObserved per execution
   4. Remove conditional suppression paths

## BLOCKED
- Full verification (requires runtime emission fixed by executor A)
- Duplicate fanout cleanup (requires canonical dispatch pipeline)


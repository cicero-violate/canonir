# EXECUTOR B PLAN (SEMANTIC AUTHORITY + INVARIANTS)

## READY NOW (MAX 5)

1. Eliminate RequestDispatch from ALL layers (CRITICAL SPEC VIOLATION)
   1. Run rg -n "RequestDispatch" canon-utils
   2. Identify EventKind definition and all propagation paths
   3. Remove RequestDispatch from EventKind
   4. Remove ALL match arms, emitters, and consumers
   5. Replace with RouteSelected-driven canonical flow

2. Enforce SemanticStateSummary as sole routing authority
   1. Open canon-route/src/policy.rs
   2. Run rg -n "scheduler_len|planned_pending|planned_count" canon-utils
   3. Identify all queue-driven decision branches
   4. Replace with SemanticStateSummary predicates ONLY

3. Remove scheduler_len from ALL decision layers
   1. Open canon-mini-agent/src/plan.rs
   2. Inspect canon-invariant and context layers
   3. Remove scheduler_len from decision logic
   4. Retain only telemetry/mirror usage

4. Remove executor-level routing decisions
   1. Open canon-route/src/executor.rs
   2. Identify all branching affecting routing
   3. Remove overrides and local decisions
   4. Ensure executor only executes policy output

5. Restore DECIDE + ROUTE trace coverage
   1. Ensure every decision emits DECIDE (canon-invariant)
   2. Ensure every RouteSelected emits ROUTE
   3. Audit early returns for missing traces

## BLOCKED
- Full verification (requires runtime emission fixed by executor A)
- Duplicate fanout cleanup (requires canonical dispatch pipeline)

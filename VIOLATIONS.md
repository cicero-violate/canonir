# Violations

## 1. Runtime not participating in event system (CRITICAL)
- Evidence:
  - actors = {"rustc": ...} only
  - No runtime actor present in event log
  - No runtime_started or tick events
- Issue:
  - Canonical runtime is not emitting or recording events
  - Entire system operates outside event-sourced control
- Required fix:
  - Ensure runtime bootstrap emits runtime_started
  - Add tick driver that emits runtime/tick events
  - Ensure runtime actor writes to event log

## 2. Canonical control chain not initiated (CRITICAL)
- Evidence:
  - No decision, route, dispatch, observe, or loop_observed events
- Issue:
  - Spec invariant (state → decision → transition → event log) completely broken
- Required fix:
  - Restore state → decision emission
  - Ensure each tick produces at least one decision
  - Add fail-fast if no decision events occur

## 3. RuntimeEvent emission/ingestion absent (CRITICAL)
- Evidence:
  - No RuntimeEvent present in logs
  - Only rustc-originated events recorded
- Issue:
  - Runtime is not acting as an event producer
- Required fix:
  - Identify and fix RuntimeEvent emission entrypoint
  - Ensure emitter is wired into runtime loop
  - Validate ingestion pipeline delivers runtime events

## 4. Routing layer never executes (CRITICAL)
- Evidence:
  - route_events_present = 0
  - No RouteExecutor activity
- Issue:
  - Routing cannot occur without decision events
- Required fix:
  - Restore decision stage first
  - Then enforce routing from SemanticStateSummary

## 5. Canonical loop fully inactive (CRITICAL)
- Evidence:
  - No observe or LoopObserved events
  - No downstream stages (plan/act/verify)
- Issue:
  - Loop never entered
- Required fix:
  - Restore pipeline entry before loop-level fixes

## 6. System operating outside canonical architecture (CRITICAL)
- Evidence:
  - Only rustc events present
  - No canonical control-flow events
- Issue:
  - System not using event-sourced execution model required by spec
- Required fix:
  - Ensure all control-flow is event-driven
  - Eliminate non-event-driven execution paths

## 7. System not spec-compliant
- Evidence:
  - No canonical stages executing
- Issue:
  - Core invariant (state → decision → route → loop → plan → act → verify) completely broken
- Required fix:
  - Restore full canonical pipeline starting from runtime event emission

# Violations

## 1. Runtime validation objectives not implemented (CRITICAL)
- Evidence:
  - OBJECTIVES.md requires validation of EventBus delivery, per-cycle guarantees, determinism, and async propagation
  - No instrumentation, counters, or verification logic present in runtime or tests
- Issue:
  - System correctness under execution is unproven
  - Violates OBJECTIVES.md requirement to validate behavior, not just architecture
- Required fix:
  - Add instrumentation for emitted vs received events
  - Add per-cycle tracking (decision count, RouteSelected presence)
  - Add validation checks or tests for objectives

## 2. EventBus delivery not verified (HIGH)
- Evidence:
  - No mechanism comparing emitted vs consumed events
  - delivered counter increments only on Error outcomes, not on every consumer delivery
  - consumer lock failure (`if let Ok(...)`) silently skips delivery
- Issue:
  - Cannot prove no-drop invariant at runtime
  - Delivery accounting is incorrect and incomplete
  - Consumers may be skipped without visibility
- Required fix:
  - Add logging or counters per event_id across consumers
  - Validate equality of emitted vs received sets
  - Increment delivery counter for every successful consumer invocation
  - Fail or log when a consumer lock cannot be acquired
  - Add assertion: delivered == sync_consumers_len (or explicit accounting)

## 3. Per-cycle guarantees not enforced or validated (HIGH)
- Evidence:
  - No cycle-level tracking of Tick → RouteTick → Decision → RouteSelected
- Issue:
  - Cannot guarantee loop integrity
- Required fix:
  - Introduce cycle_id tracking and assertions

## 4. Exactly-one decision per cycle not validated (HIGH)
- Evidence:
  - No counters or assertions on decision frequency
- Issue:
  - Potential for duplicate or missing decisions
- Required fix:
  - Track decisions per cycle and enforce invariant

## 5. Deterministic decision behavior not verified (MEDIUM)
- Evidence:
  - No replay or comparison mechanism for identical semantic input
- Issue:
  - Cannot prove determinism requirement
- Required fix:
  - Add deterministic replay validation

## 6. Async event propagation not verified (MEDIUM)
- Evidence:
  - No validation that async events re-enter loop and affect decisions
- Issue:
  - Possible silent loss or non-observation of async events
- Required fix:
  - Add tracing from async emission → EventBus → loop consumption

## 7. No-hidden-routing-paths objective not fully verified (MEDIUM)
- Evidence:
  - No explicit audit confirming all RouteSelected emissions originate from decision()
- Issue:
  - Potential for hidden routing paths
- Required fix:
  - Search and assert single routing path

## 8. Runtime loop not exercised (CRITICAL)
- Evidence:
  - Runtime executed in `--once` mode exits before main loop
  - No evidence of Tick → RouteTick → Decision → RouteSelected progression under execution
- Issue:
  - Violates OBJECTIVES.md requirement to validate behavior under execution
  - No proof that control loop actually produces lawful transitions
- Required fix:
  - Run full loop (non-once mode) with tracing enabled
  - Capture and verify full per-cycle progression

## 9. Exactly-one decision invariant not enforced (CRITICAL)
- Evidence:
  - No runtime counter or assertion enforcing 1 decision per cycle
  - Only isolated policy tests exist
- Issue:
  - Violates deterministic control requirement in SPEC.md
  - System may emit 0 or multiple decisions per cycle without detection
- Required fix:
  - Add per-cycle decision counter
  - Assert exactly one decision per cycle at runtime

## 10. Determinism not proven at runtime (HIGH)
- Evidence:
  - Determinism only validated via unit tests in policy layer
  - No runtime replay or equivalence validation
- Issue:
  - SPEC.md requires deterministic routing from semantic state
  - Runtime behavior may diverge due to ordering, async, or state drift
- Required fix:
  - Add runtime replay or snapshot comparison for identical SemanticStateSummary
  - Assert identical RouteSelected outcomes

## 11. EventBus still allows silent delivery gaps (HIGH)
- Evidence:
  - `if let Ok(mut locked)` allows consumer lock failure to skip delivery silently
  - No assertion that all consumers receive each event
- Issue:
  - Violates EventBus integrity objective (no-drop requirement)
  - Delivery completeness is not guaranteed
- Required fix:
  - Fail or log on lock acquisition failure
  - Track delivery per consumer and assert completeness

## 12. Async event propagation not verified (HIGH)
- Evidence:
  - No runtime evidence that async events re-enter loop and affect decisions
- Issue:
  - Violates OBJECTIVES.md async propagation requirement
  - System may drop or ignore async events silently
- Required fix:
  - Add tracing from async emit → bus → loop
  - Validate observation and effect on future decisions

## 8. Deterministic decision invariant violated (CRITICAL)
- Evidence:
  - Failing test: policy::tests::route_transition_rows_cover_deterministic_and_rewrite_cases
  - Panic: assertion failed: eval.deterministic.is_some() || event.is_none()
- Issue:
  - Decision system does not consistently produce deterministic evaluation results
  - Violates OBJECTIVES.md Objective 5 (Deterministic Decision Behavior)
- Required fix:
  - Ensure every decision evaluation produces deterministic output when event is present
  - Audit policy/decision evaluation path for missing deterministic assignment
  - Enforce invariant at decision boundary

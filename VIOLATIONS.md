# Violations

## 1. Build/runtime mismatch for EventBus (CRITICAL)
- Evidence:
  - register() patched with BUS REGISTER TRACE, but no trace logs appear
  - dispatch reports sync_consumers_len = 0
  - EventRuntime::new logs registrations, but consumers not present at dispatch
- Issue:
  - Running binary is not using the patched EventBus (wrong crate/version/path)
  - Consumer registration is effectively a no-op at runtime
- Required fix:
  - Ensure single EventBus implementation is used across build (no duplicate crates/paths)
  - Verify Cargo workspace resolves to intended canon-runtime crate
  - Clean/rebuild to eliminate stale artifacts (e.g., cargo clean)
  - Add invariant/log: register() must be observable in runtime logs

## 2. No consumers registered at dispatch (CRITICAL)
- Evidence:
  - sync_consumers_len = 0 during dispatch
- Issue:
  - Event bus has no active consumers → no routing/loop execution possible
- Required fix:
  - Ensure RouteExecutor and LoopStageExecutor are registered on the same bus instance used for dispatch
  - Validate registration occurs before any dispatch
  - Add fail-fast if consumer count == 0

## 3. Runtime not participating in event system (CRITICAL)
- Evidence:
  - Diagnostics show only rustc actor; no runtime_started/tick
  - No decision/route/observe/loop events
- Issue:
  - Runtime is not emitting canonical events
- Required fix:
  - Ensure runtime bootstrap emits runtime_started and periodic tick
  - Register runtime actor and emitter to tlog

## 4. Canonical control chain not initiated (CRITICAL)
- Evidence:
  - No decision, route, dispatch, observe, or loop_observed events
- Issue:
  - state → decision → transition pipeline never starts
- Required fix:
  - Construct SemanticStateSummary and emit decision each tick
  - Enforce invariant: decision per tick or fail-fast

## 5. Routing layer never executes (CRITICAL)
- Evidence:
  - route_events_present = 0
  - No RouteExecutor activity
- Issue:
  - No decision → no routing; also no consumers to receive events
- Required fix:
  - Fix consumer registration and ensure routing derives from SemanticStateSummary

## 6. Canonical loop fully inactive (CRITICAL)
- Evidence:
  - No observe or LoopObserved; no plan/act/verify
- Issue:
  - Loop never entered due to upstream failures
- Required fix:
  - Restore dispatch → route → loop after fixing EventBus linkage and runtime emission

## 7. System not spec-compliant
- Evidence:
  - Zero canonical stage execution; consumer count zero; runtime actor absent
- Issue:
  - Violates core invariant (state → decision → route → loop → plan → act → verify)
- Required fix:
  - Correct build linkage and EventBus usage, then restore full canonical pipeline

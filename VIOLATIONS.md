# Violations

## 1. EventBus build/runtime mismatch (CRITICAL)
- Evidence:
  - BUS REGISTER TRACE (in patched register()) never appears
  - BUS DISPATCH TRACE appears and shows sync_consumers_len = 0
  - EventRuntime::new logs registration loop execution
- Issue:
  - Runtime is executing a different EventBus implementation than the patched source
  - Consumer registration has no effect on the dispatching instance
- Required fix:
  - Eliminate duplicate or stale crate artifacts
  - Ensure single canonical EventBus implementation is linked
  - Perform clean rebuild (cargo clean + rebuild)
  - Verify register() logs appear at runtime

## 2. No consumers registered at dispatch (CRITICAL)
- Evidence:
  - sync_consumers_len = 0 during dispatch
- Issue:
  - No RouteExecutor or LoopStageExecutor receiving events
  - Canonical pipeline cannot execute
- Required fix:
  - Ensure registration occurs on the same EventBus instance used for dispatch
  - Add invariant: consumer count must be > 0 before dispatch

## 3. Runtime not participating in canonical event system (CRITICAL)
- Evidence:
  - No decision, route, observe, or loop events
  - Dispatch executes but has no consumers
- Issue:
  - Runtime emits events into a non-functional pipeline
- Required fix:
  - Fix EventBus linkage so emitted events reach registered consumers
  - Ensure runtime actor participates in event log

## 4. Canonical control-flow chain not executed (CRITICAL)
- Evidence:
  - No decision → route → loop → plan → act → verify stages
- Issue:
  - Entire control chain blocked at dispatch due to missing consumers
- Required fix:
  - Restore functional EventBus so routing and loop stages receive events

## 5. System not spec-compliant
- Evidence:
  - Dispatch runs with zero consumers
  - No canonical stages execute
- Issue:
  - Violates core invariant: state → decision → route → loop → plan → act → verify
- Required fix:
  - Resolve build/runtime mismatch and restore full event-driven pipeline

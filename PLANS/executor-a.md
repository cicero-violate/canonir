# EXECUTOR A PLAN (CONTROL-FLOW ROOT FIXES)

## READY NOW (MAX 5)

1. Guarantee LoopObserved emission (ROOT INVARIANT)
   1. Open canon-loop/src/stage/observe.rs
   2. Enforce strict single-exit structure
   3. Remove ALL early returns before emission
   4. Add assertion: LoopObserved emitted exactly once

2. Remove observe suppression paths (CRITICAL BLOCKER)
   1. Audit ctx.last_observed_tick and related guards
   2. Ensure guards DO NOT skip emission
   3. Verify emission occurs even when no state change

3. Eliminate RequestDispatch (SYNTHETIC ROOT)
   1. Run rg -n "RequestDispatch" canon-utils
   2. Remove from EventKind + all emitters/consumers
   3. Ensure RouteSelected is sole dispatch mechanism

4. Remove dispatch duplication / replay
   1. Inspect canon-runtime dispatch paths
   2. Remove RouteSelected replay/double-processing
   3. Enforce invariant: one event_id → one dispatch

5. Enforce lifecycle completion (FAIL-FAST)
   1. Trace emitted → routed → executed → discharged
   2. Add fail-fast on missing discharge
   3. Ensure no partial lifecycle exits exist

## BLOCKED
- Semantic convergence + trace validation (requires invariant restoration)

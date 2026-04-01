# EXECUTOR A PLAN (CONTROL-FLOW ROOT FIXES)

## READY NOW (MAX 5)

1. Guarantee LoopObserved emission (TOP PRIORITY)
   1. Open canon-loop/src/stage/observe.rs
   2. Collapse ALL control paths into a single exit
   3. Ensure LoopObserved is emitted unconditionally
   4. Add assertion: no return without emission

2. Remove control-flow bypass paths (ROOT CAUSE)
   1. Eliminate early returns and implicit fallthrough
   2. Enforce invariant checkpoints before exit
   3. Ensure emission cannot be skipped

3. Eliminate synthetic dispatch (STRUCTURAL FAILURE)
   1. Run rg -n "RequestDispatch" canon-utils
   2. Remove RequestDispatch from all layers
   3. Ensure RouteSelected is sole dispatch path

4. Normalize dispatch pipeline (SINGLE PATH)
   1. Trace emit → route → execute
   2. Remove duplicate replay / fanout paths
   3. Enforce one dispatch per event_id

5. Enforce lifecycle completion (EVENT INTEGRITY)
   1. Trace event lifecycle
   2. Ensure emitted → routed → executed → discharged
   3. Add fail-fast on discharge gaps

## BLOCKED
- Semantic convergence + trace validation (requires invariant restoration)

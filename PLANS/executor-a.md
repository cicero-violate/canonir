# EXECUTOR A PLAN (CONTROL-FLOW ROOT FIXES)

## READY NOW (MAX 5)

1. Restore runtime emission (HARD BLOCKER — NO VALIDATION WITHOUT THIS)
   1. Trace runtime loop in canon-runtime/src/lib.rs
   2. Identify event log append path
   3. Insert trace at write boundary
   4. Run runtime and verify log growth
   5. If no growth → STOP and fix immediately

2. Enforce heartbeat + fail-fast (OBSERVABILITY GUARANTEE)
   1. Emit ≥1 event per loop cycle
   2. Add invariant: no write within threshold ⇒ fail-fast
   3. Ensure supervisor cannot silently stall

3. Eliminate synthetic dispatch paths (PRIMARY CONTROL-FLOW BREAK)
   1. Run rg -n "dispatch|synthetic" canon-utils
   2. Enumerate ALL dispatch entrypoints
   3. Remove non-canonical paths
   4. Enforce exactly-one dispatch per event_id

4. Enforce canonical dispatch pipeline (SINGLE PATH)
   1. Trace emit → route → execute path
   2. Remove parallel/bypass flows
   3. Add invariant: single pipeline per event_id

5. Enforce lifecycle completion (CLOSURE INVARIANT)
   1. Trace event lifecycle
   2. Ensure emitted → routed → executed → discharged
   3. Add assertion per event_id

## BLOCKED
- Semantic + trace validation (requires emission + dispatch normalization)

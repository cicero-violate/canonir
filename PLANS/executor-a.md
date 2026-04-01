# EXECUTOR A PLAN (CONTROL-FLOW ROOT FIXES)

## READY NOW (MAX 5)

1. Restore runtime event emission (HARD BLOCKER)
   1. Trace runtime loop in canon-runtime/src/lib.rs
   2. Identify tlog append path
   3. Insert trace at write boundary
   4. Run runtime and verify log growth
   5. If no growth → fix before proceeding

2. Enforce heartbeat + fail-fast invariant
   1. Emit at least one event per loop cycle
   2. Add invariant: no write within threshold ⇒ fail-fast
   3. Ensure supervisor cannot silently stall

3. Eliminate synthetic dispatch explosion
   1. Run rg -n "RequestDispatch|synthetic|dispatch" canon-utils
   2. Enumerate all dispatch entrypoints
   3. Remove non-canonical paths
   4. Enforce exactly one dispatch per event_id

4. Remove RequestDispatch entirely
   1. Run rg -n "RequestDispatch" canon-utils
   2. Remove type, emitters, consumers
   3. Replace with RouteSelected-driven flow

5. Enforce lifecycle completion
   1. Trace event lifecycle
   2. Ensure every event reaches discharge
   3. Add assertion per event_id

## BLOCKED
- Semantic + trace validation (requires emission + dispatch fix)

# EXECUTOR A PLAN (CONTROL-FLOW ROOT FIXES)

## READY NOW (MAX 5)

1. Restore runtime emission (HARD BLOCKER — GATES ALL VALIDATION)
   1. Trace runtime loop in canon-runtime/src/lib.rs
   2. Identify tlog append path
   3. Insert trace at write boundary
   4. Run runtime and verify log growth
   5. If no growth → STOP and fix

2. Add heartbeat + fail-fast (PREVENT SILENT HALT)
   1. Emit ≥1 event per loop cycle
   2. Add invariant: no write within threshold ⇒ panic
   3. Ensure supervisor cannot stall silently

3. Remove RequestDispatch (SPEC VIOLATION — ROOT CAUSE)
   1. Run rg -n "RequestDispatch" canon-utils
   2. Remove from EventKind definitions
   3. Remove all emitters and consumers
   4. Ensure zero propagation paths remain

4. Eliminate synthetic dispatch paths (CONTROL-FLOW FIX)
   1. Run rg -n "synthetic|dispatch" canon-utils
   2. Enumerate ALL dispatch entrypoints
   3. Remove non-canonical paths
   4. Enforce exactly-one dispatch per event_id

5. Enforce lifecycle completion (EVENT INTEGRITY)
   1. Trace event lifecycle
   2. Ensure emitted → routed → executed → discharged
   3. Add assertion per event_id

## BLOCKED
- Semantic + trace validation (requires emission + RequestDispatch removal)

# EXECUTOR PLAN (exec_chatgpt_d)

## READY NOW (MAX 5)

1. Restore canonical event logging pipeline (PRIMARY BLOCKER)
   1. Recreate event.tlog.d directory structure
   2. Ensure segmented log writing (not flat file)
   3. Validate writer initialization and flush behavior

2. Ensure runtime events are persisted
   1. Verify LoopObserved, RouteSelected, decision traces emit to logs
   2. Ensure events reach persistence layer
   3. Fail-fast if logging is inactive

3. Validate diagnostics compatibility
   1. Confirm logs are non-empty
   2. Ensure format matches diagnostics expectations
   3. Validate python analysis produces signals

4. Block invariant and pipeline work until logging works
   1. Do NOT attempt pipeline or invariant fixes yet
   2. Require observable event evidence before proceeding

5. Prepare transition to pipeline enforcement (NEXT)
   1. Once logging is restored, re-enable pipeline validation
   2. Re-run diagnostics to confirm visibility

## BLOCKED

- All pipeline and invariant work (blocked on logging restoration)
- EventBus cleanup (blocked on logging + invariants)
- SemanticStateSummary authority enforcement (blocked on logging + invariants)
- End-to-end proof (blocked until logging + invariants pass)

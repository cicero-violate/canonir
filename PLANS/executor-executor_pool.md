# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 5)

1. ENFORCE STATE → DECISION (PRIMARY BLOCKER)
   - construct SemanticStateSummary at runtime start
   - trigger judgment evaluation from state
   - emit decision event (no silent pass-through)
   - fail-fast if no decision occurs

2. ENFORCE DECISION → TRANSITION
   - ensure every decision produces a canonical transition
   - emit RouteSelected (or equivalent transition event)
   - fail-fast if decision has no transition

3. ENFORCE TRANSITION → EVENT LOG
   - ensure all transitions are emitted via emit_event
   - ensure drain_emitted_events flushes to tlog
   - fail-fast if transition not recorded

4. ACTIVATE RUNTIME AS EVENT PRODUCER
   - ensure runtime_started and tick events exist
   - verify runtime participates as non-rustc actor

5. HARD GATE
   - decision > 0
   - transition > 0
   - events recorded in log

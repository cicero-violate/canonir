# Diagnostics Report

## Inputs Scanned
- event log segments in state/event_log/event.tlog.d (1 file, ~30.4MB)
- latest analysis:
  - seconds_since_last_write ≈ 10773s (CRITICAL staleness)
  - loop_observed_missing=24172
  - missing_decision_traces=5167
  - missing_route_traces=5166
  - successor_discharge_gaps=2188
  - synthetic_dispatch_signals=461
  - queue_driven_routing_signals=16

## Ranked Failures

1. Impact: critical
   Signal: Runtime observability halted
   Evidence: No log writes for ~10773 seconds
   Repair Targets:
   - canon-runtime logging loop
   - enforce heartbeat + fail-fast on stalled emission

2. Impact: critical
   Signal: Synthetic dispatch still present
   Evidence: 461 synthetic dispatch signals
   Repair Targets:
   - canon-runtime dispatch layer
   - fully eliminate RequestDispatch and synthetic paths

3. Impact: critical
   Signal: Queue-driven routing persists
   Evidence: 16 queue-driven routing signals
   Repair Targets:
   - canon-route policy + plan.rs
   - remove scheduler_len / planned_pending usage
   - enforce SemanticStateSummary-only routing

4. Impact: critical
   Signal: Missing DECIDE/ROUTE traces
   Evidence: >5k missing traces
   Repair Targets:
   - executor + invariant enforcement
   - guarantee DECIDE + ROUTE emission

5. Impact: critical
   Signal: LoopObserved invariant failure
   Evidence: 24172 missing events
   Repair Targets:
   - canon-loop observe stage
   - enforce unconditional emission

6. Impact: high
   Signal: Successor lifecycle incomplete
   Evidence: 2188 discharge gaps
   Repair Targets:
   - runtime lifecycle completion logic

## Planner Handoff

Priority order:
1. Restore event-log emission
2. Eliminate synthetic dispatch
3. Remove scheduler-derived routing inputs
4. Enforce SemanticStateSummary-only routing
5. Restore tracing invariants
6. Fix LoopObserved guarantees
7. Close lifecycle gaps

Blockers:
- No fresh logs → cannot validate fixes
- Control flow bypasses semantic state authority


# Diagnostics Report

## Inputs Scanned
- event log segments in state/event_log/event.tlog.d (1 file, ~30.4MB total)
- time-aware analysis:
  - seconds_since_last_write ≈ 6701s (**CRITICAL: runtime emission halted**)
- python structured scan:
  - loop_observed_missing=24172
  - missing_decision_traces=5167
  - missing_route_traces=5166
  - successor_discharge_gaps=2188
  - synthetic_dispatch_signals=461
  - queue_driven_routing_signals=16

## Ranked Failures

### 1. Impact: CRITICAL
Signal: Runtime observability failure (event log halted)
Evidence:
- No writes for ~6701 seconds
- Metrics frozen despite known divergence
Repair Targets:
- canon-runtime
  - enforce heartbeat emission per loop
  - fail-fast if logging stalls

### 2. Impact: CRITICAL
Signal: Synthetic dispatch explosion (non-canonical control flow)
Evidence:
- 461 synthetic dispatch signals
Repair Targets:
- canon-runtime dispatch
  - eliminate all non-canonical dispatch paths
  - enforce exactly-one dispatch per event

### 3. Impact: CRITICAL
Signal: Routing authority violation (queue-driven routing)
Evidence:
- 16 queue-driven routing signals
Repair Targets:
- canon-route + plan.rs
  - remove scheduler_len / planned_pending usage
  - enforce SemanticStateSummary-only routing

### 4. Impact: CRITICAL
Signal: Observability collapse (missing DECIDE/ROUTE traces)
Evidence:
- >5k missing traces
Repair Targets:
- canon-invariant + executor
  - enforce DECIDE + ROUTE emission invariant

### 5. Impact: CRITICAL
Signal: LoopObserved invariant failure
Evidence:
- 24k missing LoopObserved events
Repair Targets:
- canon-loop observe stage
  - enforce unconditional LoopObserved emission

### 6. Impact: CRITICAL
Signal: Lifecycle incomplete (missing discharge)
Evidence:
- 2188 discharge gaps
Repair Targets:
- canon-runtime
  - enforce full lifecycle completion

## Planner Handoff

Priority:
1. Restore event-log emission
2. Eliminate synthetic dispatch paths
3. Enforce SemanticStateSummary authority
4. Remove queue-driven routing logic
5. Restore DECIDE/ROUTE tracing
6. Fix LoopObserved invariant
7. Enforce lifecycle completion

Blockers:
- No fresh logs → cannot validate fixes
- Control-flow fragmentation bypasses canonical logic


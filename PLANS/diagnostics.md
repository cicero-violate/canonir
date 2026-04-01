# Diagnostics Report

## Inputs Scanned
- event log segments in state/event_log/event.tlog.d (1 file, ~30.4MB total)
- time-aware analysis:
  - seconds_since_last_write ≈ 5709s (**CRITICAL: runtime emission halted**)
- python structured scan:
  - loop_observed_missing=24172
  - missing_decision_traces=5167
  - missing_route_traces=5166
  - successor_discharge_gaps=2188
  - synthetic_dispatch_signals=461
  - queue_driven_routing_signals=16
- verifier evidence:
  - RequestDispatch still present in EventKind and propagated
  - scheduler_len still present and actively updated

## Ranked Failures

### 1. Impact: CRITICAL
Signal: Runtime observability failure (event log halted)
Evidence:
- No writes for ~5709 seconds
- Large-scale invariant violations present prior to stall
Repair Targets:
- canon-runtime
  - enforce heartbeat emission per loop cycle
  - fail-fast on stalled logging

### 2. Impact: CRITICAL
Signal: RequestDispatch not eliminated (spec violation)
Evidence:
- verifier confirms presence in EventKind and propagation layers
Repair Targets:
- canon-runtime-events
  - remove RequestDispatch from EventKind
- canon-loop, canon-route, executor
  - remove all match arms and propagation paths

### 3. Impact: CRITICAL
Signal: Routing authority violation (queue-driven state persists)
Evidence:
- queue_driven_routing_signals = 16
- verifier confirms scheduler_len still exists and is updated
Repair Targets:
- canon-route/src/policy.rs
- canon-mini-agent/src/plan.rs
- context/invariant layers
  - eliminate scheduler_len and planned_pending usage
  - enforce SemanticStateSummary as sole authority

### 4. Impact: CRITICAL
Signal: Synthetic dispatch explosion (non-canonical control flow)
Evidence:
- synthetic_dispatch_signals = 461
Repair Targets:
- canon-runtime dispatch
  - eliminate all alternate dispatch paths
  - enforce single canonical pipeline

### 5. Impact: CRITICAL
Signal: Observability collapse (missing DECIDE/ROUTE traces)
Evidence:
- >5k missing traces
Repair Targets:
- canon-invariant + executor
  - enforce DECIDE + ROUTE emission invariant

### 6. Impact: CRITICAL
Signal: LoopObserved invariant failure
Evidence:
- 24k missing LoopObserved events
Repair Targets:
- canon-loop observe stage
  - enforce unconditional emission

### 7. Impact: CRITICAL
Signal: Lifecycle incomplete (missing discharge)
Evidence:
- 2188 discharge gaps
Repair Targets:
- canon-runtime
  - enforce full lifecycle completion

## Systemic Insight

- System shows **combined architectural + runtime failure**:
  - Runtime has halted (no observability)
  - Core spec invariants (RequestDispatch removal, semantic routing) not satisfied
  - Control-flow fragmentation persists (synthetic dispatch)
- SemanticStateSummary is not authoritative in execution

## Planner Handoff

Priority:
1. Restore runtime emission (unblock observability)
2. Fully eliminate RequestDispatch from all layers
3. Remove scheduler_len and queue-derived routing state
4. Enforce SemanticStateSummary as sole routing authority
5. Eliminate synthetic dispatch paths
6. Restore DECIDE/ROUTE tracing
7. Fix LoopObserved invariant
8. Enforce lifecycle completion

Blockers:
- No fresh logs → cannot validate fixes
- Architectural violations (RequestDispatch + scheduler_len) still present in code


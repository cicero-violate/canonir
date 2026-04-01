# Diagnostics Report

## Inputs Scanned
- event log segments in state/event_log/event.tlog.d (1 file, ~380KB total)
- python structured scan (latest):
  - latest_mtime_delta_sec ≈ 347s (log is **stale relative to current cycle**)
  - loop_observed_missing=77
  - observe_recovery_missing=0
  - act_with_empty_scheduler=2
  - missing_decision_traces=61
  - missing_route_traces=60
  - successor_discharge_gaps=38
  - duplicate_fanout_signals=1
  - scheduler_drift_signals=1
  - synthetic_dispatch_signals=2

## Time / Freshness Assessment
- Event log **not updated recently (~347s old)**
- Signals may reflect **previous cycle state**, not current execution
- Risk: partial or stale conclusions if system has progressed

Implication:
- Failures below are **confirmed historically**, but may not reflect most recent fixes
- However, **monotonic growth patterns across prior runs strongly indicate unresolved systemic issues**

## Ranked Failures

### 1. Impact: HIGH
Signal: Missing DECIDE + ROUTE trace coverage (systemic, scaling)
Evidence:
- missing_decision_traces = 61
- missing_route_traces = 60
- historically increasing monotonically
Repair Targets:
- canon-invariant/src/lib.rs
  - emit DECIDE(trace_id, decision, full inputs)
- canon-route/src/executor.rs
  - emit ROUTE(trace_id, decision→route mapping)
- invariant:
  - route_selected ⇒ exactly one DECIDE + one ROUTE

### 2. Impact: HIGH
Signal: LoopObserved emission missing / non-deterministic
Evidence:
- loop_observed_missing = 77 (largest growing metric)
Repair Targets:
- canon-loop/src/stage/observe.rs
  - enforce unconditional LoopObserved emission
  - eliminate all conditional skips

### 3. Impact: HIGH
Signal: Event lifecycle incomplete (successor discharge gaps)
Evidence:
- successor_discharge_gaps = 38
Repair Targets:
- canon-runtime
  - enforce lifecycle completion: emitted → routed → executed → discharged
  - add explicit discharge assertion

### 4. Impact: HIGH
Signal: Synthetic dispatch paths (control-flow corruption)
Evidence:
- synthetic_dispatch_signals = 2
Repair Targets:
- canon-runtime dispatch layer
  - eliminate synthetic fanout paths
  - enforce single canonical dispatch per event

### 5. Impact: HIGH
Signal: Act executed with empty scheduler
Evidence:
- act_with_empty_scheduler = 2
Repair Targets:
- canon-invariant/src/lib.rs
  - enforce scheduler_len == 0 ⇒ Observe
- canon-route executor
  - block Act when scheduler empty

### 6. Impact: MEDIUM
Signal: Scheduler drift
Evidence:
- scheduler_drift_signals = 1
Repair Targets:
- canon-loop context
  - enforce single source of truth
  - validate scheduler before decision

### 7. Impact: MEDIUM
Signal: Duplicate dispatch
Evidence:
- duplicate_fanout_signals = 1
Repair Targets:
- runtime dispatch
  - idempotent dispatch (event_id guard)

### 8. Impact: MEDIUM
Signal: Observe recovery path missing
Evidence:
- no recovery traces
Repair Targets:
- canon-loop observe stage
  - implement explicit recovery state + trace

## Temporal Diagnostics Insights

- Failures (LoopObserved, trace gaps, discharge gaps) show **consistent monotonic growth across runs**
- Indicates **systemic invariant violations**, not transient bugs
- No evidence of regression reversal → fixes not taking effect or not executed

## Planner Handoff

Highest priority:
1. Enforce DECIDE + ROUTE trace completeness
2. Make LoopObserved emission unconditional
3. Fix event lifecycle (guarantee discharge)
4. Remove synthetic dispatch paths
5. Enforce scheduler invariant (no Act on empty)

Secondary:
6. Fix scheduler drift
7. Remove duplicate dispatch
8. Implement observe recovery

Blockers:
- Event log staleness (~347s) → need fresh cycle validation
- Persistent monotonic failure growth → indicates fixes not wired into execution path


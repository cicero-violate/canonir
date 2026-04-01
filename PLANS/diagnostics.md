# Diagnostics Report

## Inputs Scanned
- event log segments in state/event_log/event.tlog.d (1 file, ~175KB total)
- python structured scan (latest):
  - loop_observed_missing=24
  - observe_recovery_missing=0
  - act_with_empty_scheduler=2
  - missing_decision_traces=28
  - missing_route_traces=28
  - successor_discharge_gaps=10
  - duplicate_fanout_signals=1
- canon system focus: canon-route, canon-loop, canon-runtime

## Ranked Failures

### 1. Impact: HIGH
Signal: Missing DECIDE + ROUTE trace coverage
Evidence:
- missing_decision_traces = 28
- missing_route_traces = 28
- systematic absence tied to route_selected events
Repair Targets:
- canon-invariant/src/lib.rs
  - emit DECIDE trace with trace_id + full decision payload
- canon-route/src/executor.rs
  - emit ROUTE trace for every route_selected
- global invariant:
  - route_selected MUST imply {DECIDE, ROUTE} traces (1:1 coverage)
  - enforce via assertion or verifier hook

### 2. Impact: HIGH
Signal: LoopObserved emission missing / conditional
Evidence:
- loop_observed_missing = 24 (increasing)
- indicates non-deterministic Observe stage emission
Repair Targets:
- canon-loop/src/stage/observe.rs
  - make LoopObserved emission unconditional
  - ensure every Observe transition produces event
  - remove branching that skips emission

### 3. Impact: HIGH
Signal: Act executed with empty scheduler
Evidence:
- act_with_empty_scheduler = 2
- violation of core decision invariant
Repair Targets:
- canon-invariant/src/lib.rs
  - enforce scheduler_len == 0 ⇒ Decision::Observe
- canon-route/src/executor.rs
  - hard-block Act routing when scheduler empty
- canon-loop/src/context.rs
  - validate scheduler_len before decision

### 4. Impact: HIGH
Signal: Successor discharge gaps (event lifecycle incomplete)
Evidence:
- successor_discharge_gaps = 10
- events not properly finalized/discharged
Repair Targets:
- canon-runtime / event system
  - enforce invariant: every event must reach discharged state
  - add explicit discharge step or confirmation
  - audit lifecycle: emit → route → act → discharge

### 5. Impact: MEDIUM
Signal: Duplicate dispatch / fanout
Evidence:
- duplicate_fanout_signals = 1
Repair Targets:
- canon-runtime / dispatch layer
  - enforce single-dispatch per event invariant
  - deduplicate dispatch paths
  - add idempotency guard (event_id-based)

### 6. Impact: MEDIUM
Signal: Observe recovery path still implicit
Evidence:
- observe_recovery_missing = 0 (no explicit markers)
- verifier indicates missing explicit implementation
Repair Targets:
- canon-loop/src/stage/observe.rs
  - implement explicit recovery path
  - emit recovery-specific trace/log
  - ensure deterministic transition semantics

## Planner Handoff

Highest-value repair targets:
1. Enforce full DECIDE + ROUTE trace coverage (critical observability gap)
2. Make LoopObserved emission unconditional
3. Enforce scheduler_len == 0 ⇒ Observe invariant
4. Fix event lifecycle: guarantee successor discharge

Secondary:
5. Eliminate duplicate dispatch/fanout
6. Implement explicit observe recovery path

Blockers / Gaps:
- Trace coverage regression growing (28 missing)
- Observe stage nondeterministic (24 misses)
- Event lifecycle incomplete (discharge gaps present)
- Control-flow invariant violations persist

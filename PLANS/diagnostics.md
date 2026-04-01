# Diagnostics Report

## Inputs Scanned
- event log segments (state/event_log/event.tlog.d)
- latest python analysis (current cycle)
- verifier summary (lane_a)
- prior inspection of canon-loop, canon-route, canon-runtime

## Ranked Failures

### 1. Impact: CRITICAL
Signal: LoopObserved invariant failure breaks routing entrypoint
Evidence:
- 24,254 missing LoopObserved events (stable across cycles)
- LoopObserved → RouteSelected is required control-flow edge
Repair Targets:
- canon-loop/src/stage/observe.rs
  - enforce single-exit structure with guaranteed LoopObserved emission
  - add runtime assertion ensuring emission occurred exactly once
  - eliminate implicit fallthrough and hidden bypass paths

### 2. Impact: CRITICAL
Signal: Decision→Route invariant collapse
Evidence:
- 5,167 missing decision traces
- 5,166 missing route traces
Repair Targets:
- canon-route/src/executor.rs
  - replace debug_assert!(true, ...) with enforced invariant
  - block route emission if decision trace is missing

### 3. Impact: CRITICAL
Signal: SemanticStateSummary is not sole routing authority
Evidence:
- 16 queue-driven routing signals
- verifier confirms continued use of scheduler_len / planned_pending
Repair Targets:
- canon-route policy + executor
  - remove all scheduler_len / planned_pending dependencies
  - enforce routing derived exclusively from SemanticStateSummary

### 4. Impact: CRITICAL
Signal: RequestDispatch / synthetic dispatch not fully removed
Evidence:
- 461 synthetic dispatch signals
- verifier explicitly reports incomplete RequestDispatch removal
Repair Targets:
- canon-runtime dispatch layer
  - fully remove RequestDispatch paths
  - eliminate synthetic fanout and replay-based dispatch

### 5. Impact: CRITICAL
Signal: Invariants detected but not enforced
Evidence:
- 1,278 invariant/error lines
- runtime continues execution despite violations
Repair Targets:
- canon-invariant
  - convert invariant violations into fail-fast behavior
  - halt execution when invariants are violated

### 6. Impact: HIGH
Signal: Successor lifecycle incomplete
Evidence:
- 2,188 successor_discharge_gaps
Repair Targets:
- canon-runtime lifecycle
  - enforce exactly-once successor discharge
  - remove lifecycle bypass cases

### 7. Impact: HIGH
Signal: Persistent missing/duplicate patterns
Evidence:
- "missing": 500 occurrences
- "duplicate": 2 occurrences
Repair Targets:
- runtime control-flow
  - eliminate early exits and implicit bypass paths
  - enforce invariant checkpoints on all exits

## Planner Handoff

Priority order:
1. Guarantee LoopObserved emission (fix observe control flow)
2. Enforce decision→route invariant strictly
3. Remove queue-derived routing inputs (semantic-only authority)
4. Fully eliminate RequestDispatch and synthetic dispatch
5. Enforce fail-fast invariant behavior
6. Fix successor lifecycle completion

Blockers:
- System is active but not converging (metrics unchanged across cycles)
- Routing/control flow still partially driven by non-semantic state
- RequestDispatch removal incomplete (per verifier)
- Invariants are informational only and not enforced


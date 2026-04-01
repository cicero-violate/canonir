# Diagnostics Report

## Inputs Scanned
- event log segments (state/event_log/event.tlog.d)
- latest structured python analysis (current cycle)
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
  - add runtime assertion ensuring emission occurred
  - eliminate implicit fallthrough reliance

### 2. Impact: CRITICAL
Signal: Decision→Route invariant collapse
Evidence:
- 5,167 missing decision traces
- 5,166 missing route traces
Repair Targets:
- canon-route/src/executor.rs
  - replace debug_assert!(true, ...) with enforced invariant
  - block route emission without decision trace

### 3. Impact: CRITICAL
Signal: Routing authority still influenced by non-semantic state
Evidence:
- 16 queue-driven routing signals
- presence of tick/hash gating in LoopContext
Repair Targets:
- canon-route + canon-loop context
  - remove scheduler_len / planned_pending / tick/hash gating
  - derive routing exclusively from SemanticStateSummary

### 4. Impact: CRITICAL
Signal: Synthetic dispatch bypass persists
Evidence:
- 461 synthetic dispatch signals
Repair Targets:
- canon-runtime
  - remove RequestDispatch entirely
  - eliminate replay duplication paths

### 5. Impact: CRITICAL
Signal: Invariants detected but not enforced
Evidence:
- 1,278 invariant/error lines
- runtime continues execution under violation
Repair Targets:
- canon-invariant
  - convert invariant violations into fail-fast behavior
  - halt execution on violation

### 6. Impact: HIGH
Signal: Successor lifecycle incomplete
Evidence:
- 2,188 successor_discharge_gaps
Repair Targets:
- canon-runtime lifecycle
  - enforce exactly-once successor discharge

### 7. Impact: HIGH
Signal: Persistent missing/duplicate patterns
Evidence:
- "missing": 500 occurrences
- "duplicate": 2 occurrences
Repair Targets:
- runtime control flow
  - eliminate bypass paths (early exits / fallthrough)
  - enforce invariant checkpoints on all exits

## Planner Handoff

Priority order:
1. Guarantee LoopObserved emission
2. Enforce decision→route invariant
3. Remove queue-derived routing inputs
4. Eliminate synthetic dispatch
5. Enforce fail-fast invariant behavior
6. Fix successor lifecycle

Blockers:
- System is active but not converging (metrics unchanged)
- Routing/control flow still partially derived from non-semantic state
- Invariants are informational only and not enforced


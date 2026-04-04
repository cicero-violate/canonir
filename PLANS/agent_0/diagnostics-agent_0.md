# Diagnostics Report

## Inputs Scanned
- Event log segments (full history + latest samples)
- VIOLATIONS.md
- runtime_output.txt traces
- canon-runtime source (append + emitter + wire conversion)

Key observation: ZERO control events exist in tlog despite confirmed runtime emission activity; events are emitted but dropped before persistence

---

## Ranked Failures

### 1. Impact: CRITICAL
Signal: Events are emitted but never persisted to tlog

Evidence:
- Event log contains ZERO control events (Tick, RouteSelected, etc.)
- Runtime traces show append attempts (APPEND ENTRY / TLOG ATTEMPT)
- '[INIT GUARD HIT] tlog_path=None dropping kind=...'
- '[NO WRITER] tlog_writer is None at append time'
- emit_with_parents(..., vec![]) observed in multiple call sites
- Concrete offending call sites:
  - canon-utils/canon-loop/src/context.rs:349
    - emitter.emit_with_parents(event, vec![], ...)
  - canon-utils/canon-runtime/src/bin/event_runtime.rs:628
    - emit_with_parents(PromptLoaded, vec![], ...)

Root Cause (COMPOUND FAILURE):
1) Writer Initialization Failure
- tlog_path is None during early emissions
- tlog_writer is not constructed before append_runtime_event
- INIT GUARD and NO WRITER paths drop events

2) Causal Chain Violation
- emit_with_parents(..., vec![]) used in:
  - canon-loop/src/context.rs
  - event_runtime.rs (PromptLoaded)
- runtime_event_to_wire rejects non-root events with empty parent_ids

These combine to ensure ALL control events are dropped.

Repair Targets:
- canon-runtime:
  - initialize tlog_path BEFORE any emission
  - construct tlog_writer before runtime loop starts
  - remove silent INIT GUARD / NO WRITER drops → fail fast instead
- emission sites (GLOBAL):
  - eliminate emit_with_parents(..., vec![])
  - enforce parent_ids propagation from trigger event
- invariants:
  - fail-fast if non-root event has empty parent_ids

Concrete Violations (confirmed):
- canon-utils/canon-loop/src/context.rs:349
  - emit_with_parents(..., vec![])
- canon-utils/canon-runtime/src/bin/event_runtime.rs:628
  - emit_with_parents(..., vec![])

These sites directly violate causal chain requirements and must be fixed first.

---

### 2. Impact: CRITICAL
Signal: Runtime loop executes but produces no persisted events

Evidence:
- runtime_output.txt shows dispatch + emission traces
- BUT tlog contains zero control events

Root Cause:
- Events rejected during wire conversion due to missing parent_ids

Repair Targets:
- canon-loop:
  - derive decision from SemanticStateSummary
  - emit decision event per tick

---

### 3. Impact: CRITICAL
Signal: State → Decision → Routing pipeline absent

Evidence:
- decision = 0
- route_selected = 0

Root Cause:
- upstream persistence failure blocks all pipeline stages

Repair Targets:
- canon-route:
  - ensure decision produces RouteSelected
  - enforce decision → transition invariant

---

### 4. Impact: HIGH
Signal: Violations.md not grounded in current tlog

Evidence:
- violations reference RouteSelected duplication but none exist in log

Root Cause:
- diagnostics based on stale or non-persisted traces

Repair Targets:
- require tlog-backed evidence for all violations

---

### 5. Impact: HIGH
Signal: System appears non-event-sourced

Evidence:
- only rustc/tooling events present

Root Cause:
- persistence failure hides actual runtime behavior

Repair Targets:
- restore full event pipeline visibility

---

## Planner Handoff

### Priority Order
1. Fix parent_ids propagation (ROOT BLOCKER)
2. Eliminate emit_located usage
3. Enforce emit_with_parents everywhere
4. Verify events pass wire conversion
5. Then restore decision → routing → loop stages

### Key Insight

System DOES execute runtime loop and emits events.
But events are dropped before persistence due to:
- missing writer initialization
- missing parent_ids

This is a combined persistence + causal chain failure.

Fix must begin with:
1) writer initialization
2) parent_ids propagation

# Diagnostics Report

## Inputs Scanned
- event log: latest segments in state/event_log/event.tlog.d
- violations: VIOLATIONS.md
- spec: PLANS/SPEC.md (state → decision → transition → event log)
- source: canon-runtime, canon-loop, canon-route, canon-mini-agent
- commands: python analysis (actors, stages, invariants)

## Ranked Failures

### 1. Impact: CRITICAL
Signal: Runtime not participating in event system; canonical pipeline completely inactive
Evidence:
- actors = {"rustc": 384} only
- only_rustc_actor = True
- no_canonical_stages = True
- No runtime_started, tick, decision, route, dispatch, observe, or loop_observed events
- repeated analyses show identical pattern (no recovery)

Root Cause:
- Runtime bootstrap is not executing canonical runtime loop
- RuntimeEvent emitter is never invoked or not constructed
- Runtime actor is not registered in event system

Repair Targets:
- canon-runtime:
  - audit program entrypoint to confirm runtime loop initialization is executed
  - ensure runtime_started event is emitted at startup
  - implement/restore tick driver to emit RuntimeEvent
  - verify EventEmitter wiring to event bus and tlog writer
  - ensure runtime actor identity is registered and visible
- invariants:
  - runtime_started must occur once per process
  - fail-fast if only rustc actor present

---

### 2. Impact: CRITICAL
Signal: state → decision never executes
Evidence:
- No decision events present
- No semantic-state-driven transitions

Root Cause:
- SemanticStateSummary is never constructed or evaluated
- Decision stage is never triggered

Repair Targets:
- canon-runtime:
  - construct SemanticStateSummary at runtime start
  - invoke decision evaluation on each tick
  - emit decision event prior to any downstream execution
- invariants:
  - decision must occur per tick

---

### 3. Impact: CRITICAL
Signal: Routing layer never executes
Evidence:
- No route events
- No RouteExecutor activity

Root Cause:
- Upstream failure: no decision → no routing

Repair Targets:
- canon-route:
  - ensure routing derives from SemanticStateSummary
  - verify RouteExecutor subscription and activation

---

### 4. Impact: HIGH
Signal: Loop execution fully inactive
Evidence:
- No observe or loop_observed events
- No plan/act/verify stages

Root Cause:
- Pipeline blocked before loop entry

Repair Targets:
- canon-loop:
  - restore route → loop execution after upstream fixes

---

### 5. Impact: HIGH
Signal: System operating outside event-sourced model
Evidence:
- Only rustc events recorded
- No control-flow events in log

Root Cause:
- Execution bypasses canonical event system entirely

Repair Targets:
- global:
  - enforce invariant: all control flow must emit events
  - audit and eliminate hidden execution paths

---

## Planner Handoff
1. Restore runtime bootstrap (PRIMARY)
2. Ensure runtime emits runtime_started and tick events
3. Emit RuntimeEvent from semantic state
4. Enable state → decision execution
5. Activate routing (decision → route)
6. Restore loop (route → observe → loop_observed)

## Blockers
- Runtime entrypoint execution path not confirmed
- Missing trace from bootstrap → emitter → event log


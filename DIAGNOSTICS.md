# Diagnostics Report

## Inputs Scanned
- event log: latest segments in state/event_log/event.tlog.d
- violations: VIOLATIONS.md
- spec: PLANS/SPEC.md (state → decision → transition → event log)
- source: canon-runtime, canon-loop, canon-route, canon-mini-agent
- commands: python analysis (actors, stages, patterns)

## Ranked Failures

### 1. Impact: CRITICAL
Signal: Runtime not participating; canonical pipeline never initiates
Evidence:
- actors = {"rustc": 343} only
- No runtime_started, tick, decision, route, dispatch, observe, or loop_observed
- repeated analyses show identical absence of canonical stages
- Spec requires event-sourced control flow

Root Cause:
- Runtime bootstrap not executing canonical runtime loop
- RuntimeEvent emission path never invoked
- No runtime actor registered in event system

Repair Targets:
- canon-runtime:
  - audit main entrypoint for runtime loop initialization
  - ensure runtime_started emitted at startup
  - implement/restore tick driver
  - verify EventEmitter wiring into event bus + tlog
  - ensure runtime actor registration
- invariants:
  - runtime_started must occur once per process
  - tick must occur continuously
  - fail-fast if only rustc actor present

---

### 2. Impact: CRITICAL
Signal: State → decision never executes
Evidence:
- No decision events
- No semantic-state-driven transitions

Root Cause:
- SemanticStateSummary never evaluated
- Decision stage never triggered

Repair Targets:
- canon-runtime:
  - construct SemanticStateSummary at startup
  - invoke decision evaluation each tick
  - emit decision event
- invariants:
  - decision must occur per tick

---

### 3. Impact: CRITICAL
Signal: Routing never executes
Evidence:
- route_events_present = 0
- no RouteExecutor activity

Root Cause:
- Upstream failure: no decision → no routing

Repair Targets:
- canon-route:
  - validate routing derives from SemanticStateSummary
  - ensure RouteExecutor subscribes correctly

---

### 4. Impact: HIGH
Signal: Loop fully inactive
Evidence:
- no observe or LoopObserved events
- no downstream plan/act/verify

Root Cause:
- Pipeline blocked before loop entry

Repair Targets:
- restore route → loop flow after upstream fix

---

### 5. Impact: HIGH
Signal: System operating outside event-sourced model
Evidence:
- Only rustc events recorded
- No control events in log

Root Cause:
- Execution bypasses canonical event system

Repair Targets:
- enforce invariant:
  - all control flow must emit events
- audit hidden execution paths

---

## Planner Handoff
1. Restore runtime bootstrap (PRIMARY)
2. Ensure runtime emits runtime_started + tick
3. Emit RuntimeEvent from semantic state
4. Enable decision stage execution
5. Verify routing activation
6. Restore loop execution

## Blockers
- Runtime entrypoint not confirmed
- Missing tracing between bootstrap and emitter


# Diagnostics Report

## Inputs Scanned
- Event log segments under state/event_log/event.tlog.d (20 recent files)
- VIOLATIONS.md
- canon-runtime emit + loop driver
- canon-route executor + decision
- loop_e2e failing tests

## Ranked Failures

### 1. Impact: CRITICAL (ROOT FAILURE)
**Signal:** No control events exist in canonical event log

**Evidence:**
- 20 log segments scanned
- Tick=0, RouteTick=0, RouteSelected=0, decision_trace=0 for all files
- Logs contain only rustc/code events despite non-zero sizes
- loop_e2e tests fail asserting these events must exist

**Root Cause:**
- Control loop not executing or not reaching persistence layer
- Serialization layer supports events → not dropped downstream
- Therefore failure is upstream:
  - emit_tick not being called continuously OR
  - runtime loop not progressing OR
  - event bus failing to append control events

**Repair Targets:**
- canon-runtime/src/lib.rs::emit_tick
  - MUST run every cycle
  - MUST emit Tick → RouteTick
  - MUST guarantee append success
- canon-runtime/src/bin/event_runtime.rs
  - Ensure infinite loop execution (no early exit)
  - Ensure emit_tick invoked repeatedly
- Event bus / writer
  - Guarantee emit_event → append always executes
  - Add read-after-write invariant

---

### 2. Impact: CRITICAL
**Signal:** RouteTick does not deterministically trigger dispatch

**Evidence:**
- executor.rs uses dispatch_in_progress gating
- try_dispatch_route can early-return

**Root Cause:**
- Executor-local state suppresses dispatch

**Repair Targets:**
- canon-route/src/executor.rs
  - Remove dispatch_in_progress gating
  - Enforce exactly-one dispatch per RouteTick

---

### 3. Impact: HIGH
**Signal:** Control authority not derived from semantic state

**Evidence:**
- reroute_requested / dispatch flags influence flow

**Repair Targets:**
- Remove executor-local control flags entirely
- Move control authority into invariant/policy layer

---

### 4. Impact: HIGH
**Signal:** decision_trace missing

**Evidence:**
- decision_trace count = 0 across all logs

**Repair Targets:**
- Emit decision_trace exactly once per RouteTick
- Include SemanticStateSummary and selected route

---

### 5. Impact: HIGH
**Signal:** Routing not fully semantic

**Evidence:**
- decision.rs incomplete mapping coverage

**Repair Targets:**
- Expand SemanticStateSummary → route mapping
- Ensure executor passes full semantic state

---

### 6. Impact: HIGH
**Signal:** Policy/invariant layer not enforced before emission

**Evidence:**
- RouteSelected not gated by invariant/policy

**Repair Targets:**
- Enforce invariant + policy BEFORE RouteSelected emission

---

## Planner Handoff

### Highest Priority Repairs
1. Restore emit_tick loop execution
2. Ensure Tick and RouteTick are persisted every cycle
3. Remove executor-local dispatch gating
4. Enforce exactly-once RouteTick → dispatch
5. Emit decision_trace per tick
6. Enforce SemanticStateSummary-driven routing
7. Integrate invariant + policy gating before emission

### Outcome Target
state → decision → RouteSelected → event log

### Status
CRITICAL FAILURE — CONTROL LOOP NOT PRODUCING CANONICAL EVENTS


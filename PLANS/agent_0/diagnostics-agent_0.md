# Diagnostics Report

## Inputs Scanned
- event log segments reviewed:
  - Latest head: `00000000000000023235.log` (`2026-04-04T10:19:17`)
  - Recent logs contain only `rustc` events
  - Token scan: no `RouteSelected`, `LoopObserved`, `Decision`, `Capability*`
- violations reviewed: `VIOLATIONS.md`
- source areas reviewed: canon-loop, canon-runtime, canon-route
- commands run: python-based log inspection

## Ranked Failures

1. Impact: CRITICAL
   Signal: Lock failure does NOT stop runtime; system continues in degraded mode without guaranteeing loop execution
   Evidence:
   - acquire_lock returns None when lock exists (event_runtime.rs:89-97)
   - main() explicitly continues when lock is None (event_runtime.rs:300-303)
   - No fail-fast or degraded-mode signal emitted
   - Event log shows zero runtime control events despite runtime binary executing
   - emit_tick only occurs inside handle_event_msg → requires event flow to trigger loop
   Repair Targets:
   - `canon-runtime/src/bin/event_runtime.rs`: treat lock acquisition failure as fatal (exit or panic)
   - Add invariant: runtime must either hold lock OR not start
   - Add health invariant: emit_tick must occur within bounded time window
   - Add explicit runtime_started / runtime_active events to canonical log

2. Impact: CRITICAL
   Signal: Runtime loop is not being driven (no event ingress → no emit_tick progression)
   Evidence:
   - handle_event_msg drives emit_tick, but requires EventMsg input
   - If watcher/bootstrap do not enqueue events, loop produces no control flow
   - Event log shows only rustc events → runtime not receiving or processing events
   Repair Targets:
   - Verify watcher thread delivers events into Q (event_runtime.rs:530+)
   - Add invariant: each cycle must produce Tick even with empty input
   - Ensure bootstrap emits initial event to trigger loop

3. Impact: CRITICAL
   Signal: EventBus delivery not enforced
   Evidence:
   - delivery gaps observable but not blocking
   Repair Targets:
   - `canon-runtime/src/bus.rs`: enforce delivery completeness invariant

4. Impact: CRITICAL
   Signal: Hook mutation/suppression not blocked
   Evidence:
   - mutation/deny only logged
   Repair Targets:
   - `canon-runtime/src/hooks.rs`: block mutation/deny for control events

5. Impact: HIGH
   Signal: Replay suppression introduces hidden control paths
   Evidence:
   - replay_suppressed events exist outside semantic state
   Repair Targets:
   - move suppression into semantic state
   - eliminate hidden branching

6. Impact: HIGH
   Signal: Per-cycle guarantees not enforced
   Evidence:
   - no Tick → Decision → RouteSelected validation
   Repair Targets:
   - add cycle_id tracking
   - enforce full cycle chain

7. Impact: HIGH
   Signal: Exactly-one decision per cycle missing
   Evidence:
   - no decision count enforcement
   Repair Targets:
   - enforce single decision invariant

## Planner Handoff
- 1. Enforce lock correctness: runtime must not run without lock
- 2. Guarantee loop heartbeat: emit_tick must occur independent of input events
- 3. Ensure event ingress path (watcher/bootstrap) feeds runtime
- 4. Re-run diagnostics after runtime produces control events
- 5. Then address semantic-state vs queue-local violations
- 3. Enforce EventBus delivery invariants
- 4. Enforce hook immutability
- 5. Remove replay suppression branching
- 6. Add cycle and decision invariants

Blockers:
- Canonical logs currently lack runtime control events, preventing validation of system behavior

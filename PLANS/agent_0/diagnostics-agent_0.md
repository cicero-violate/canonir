# Diagnostics Report

## Inputs Scanned
- event log segments reviewed:
  - freshest canonical segments `state/event_log/event.tlog.d/00000000000000008641.log` through `00000000000000008675.log`
  - additional recent windows around `00000000000000008493.log` through `00000000000000008595.log`
- violations reviewed:
  - `VIOLATIONS.md`
- source areas reviewed:
  - `canon-utils/canon-runtime/src/bus.rs`
  - `canon-utils/canon-runtime/src/lib.rs`
  - `canon-utils/canon-runtime/src/bin/event_runtime.rs`
  - `canon-utils/canon-route/src/policy.rs`
  - `canon-utils/canon-route/src/executor.rs`
- commands run:
  - repeated Python scans over latest canonical event-log segments
  - source-pattern scans for `EventBus`, `EventRuntime`, `planned_pending`, `scheduler_len`, `SemanticStateSummary`
  - `rg` scan over canon sources for bus registration / dispatch hooks

## Ranked Failures
1. Impact: high
   Signal: live canonical pipeline is inactive even though source constructs and registers a non-empty consumer set.
   Evidence:
   - freshest canonical event-log segments contain only rustc events and zero canonical control-flow events (`route_selected`, `loop_observed`, `planning_completed`, `loop_acted`, `loop_verified`, `verifier_policy_updated`, `loop_rewarded` all absent)
   - same freshest canonical segments also contain zero `BUS REGISTER TRACE`, zero `BUS DISPATCH TRACE`, and zero `[RUNTIME NEW]` signals
   - `VIOLATIONS.md` reports `sync_consumers_len = 0` during dispatch and explicitly calls out an EventBus build/runtime mismatch
   - `canon-utils/canon-runtime/src/bus.rs` shows `register()` and `dispatch()` operate on the same `EventBus.sync_consumers` vector, so the bug is not explained by the local bus implementation alone
   - `canon-utils/canon-runtime/src/lib.rs` (`EventRuntime::new`) creates a local `EventBus`, registers the provided consumers onto that bus, then stores that same bus into `EventRuntime`
   - `canon-utils/canon-runtime/src/bin/event_runtime.rs` constructs a non-empty consumer vector including `GoalGenConsumer`, `AnalystConsumer`, `RepairControlConsumer`, `RouteExecutor`, `DispatchConsumer`, `LoopStageExecutor`, `DiagnosticsConsumer`, `CapabilityExecutor`, and others before calling `EventRuntime::new(consumers)`
   - `runtime_debug.log` shows consumer enumeration from an older run, but its mtime is stale by about 11,378 seconds relative to the freshest canonical event-log segments, so it cannot override current canonical evidence
   Repair Targets:
   - `canon-utils/canon-runtime/src/bin/event_runtime.rs`
     - verify the actual launched binary and crate path match this source file
     - add startup logging to the canonical event log, not only to stderr / ad-hoc debug files
   - `canon-utils/canon-runtime/src/lib.rs`
     - add a fail-fast invariant in `EventRuntime::new` and immediately before first dispatch: registered control consumers must be `> 0`
     - emit a canonical runtime-start / runtime-registration summary event into the event log with consumer names and count
   - build/runtime linkage
     - perform clean rebuild and remove stale artifacts so the executing runtime matches current source
     - verify the launched process is writing to the same canonical `state/event_log/event.tlog.d` being inspected
   - runtime observability
     - move `BUS REGISTER TRACE` / `BUS DISPATCH TRACE` style evidence into canonical events or canonical runtime log plumbing so the active runtime path is observable from current artifacts

2. Impact: high
   Signal: route/control decisions still depend on queue-local mirrors (`planned_pending`, pending tool queues, awaiting successor state) instead of being derived solely from semantic state authority.
   Evidence:
   - canonical law says `SemanticStateSummary` is the single source of truth and queue-local counters are not authoritative unless proven derived mirrors
   - `canon-utils/canon-route/src/policy.rs` still branches on queue-local state for routing and dispatch behavior:
     - `dispatch_route_proposal(...)` uses `ctx.planned_pending == 0` in multiple route proposals
     - `event_route_proposal(...)` uses `ctx.planned_pending == 0`, `ctx.planned_pending > 0`, and `ctx.pending_tool_result_ids.is_empty()` to choose `Plan`, `Act`, `Verify`, and `Observe`
     - `evaluate_route_event_dispatch(...)` uses `planned_pending == 0` and `pending_tool_results_empty` to decide idle dispatch vs recoverable empty plan dispatch
   - comments in `canon-utils/canon-route/src/policy.rs` explicitly say "remove planned_pending dependency" and "semantic-only routing", but queue-local control conditions still remain in adjacent code paths
   - `canon-utils/canon-route/src/executor.rs` consumes batch-settled / idle-dispatch flows using `self.ctx.planned_pending` and pending tool-result emptiness before rerouting
   Repair Targets:
   - `canon-utils/canon-route/src/policy.rs`
     - remove queue-local authority from `dispatch_route_proposal(...)`
     - remove queue-local authority from `event_route_proposal(...)`
     - replace `planned_pending` / `pending_tool_result_ids.is_empty()` gating with semantic-state-derived readiness / outstanding-work facts
     - define which queue mirrors are legal derived mirrors and prove them from semantic state before use
   - `canon-utils/canon-route/src/executor.rs`
     - stop using queue emptiness as a primary reroute trigger unless the state is explicitly marked as a derived mirror from semantic state
   - invariants/tests
     - add route-policy tests that fail whenever `planned_pending`, `scheduler_len`, or pending queue emptiness directly changes route choice without semantic-state proof

3. Impact: medium
   Signal: stale ad-hoc runtime traces can contradict fresher canonical event-log evidence and mislead repair work.
   Evidence:
   - `runtime_debug.log` contains consumer enumeration, but its mtime is significantly older than the freshest canonical log segments
   - canonical event-log evidence is newer and shows zero control-flow activity
   - prompt rules explicitly require preferring latest canonical event-log segments over temp/stale traces when they disagree
   Repair Targets:
   - diagnostics/runtime tooling
     - gate trust in `runtime_debug.log`, `/tmp` traces, and similar files on freshness checks relative to canonical event-log mtimes
     - write runtime-start / consumer-registration evidence into canonical artifacts instead of ad-hoc files
   - `VIOLATIONS.md` / diagnostics flow
     - record freshness of every trace source used as evidence

4. Impact: medium
   Signal: current canonical event production is dominated by rustc capture noise, which hides whether runtime control stages are alive.
   Evidence:
   - recent windows over 24, 200, and freshest 8 segments were dominated by `rustc`, `rustc_capture_started`, `rustc_graph_artifact_written`, `rustc_capture_completed`, and `rustc_capture_failed`
   - zero route/loop control events appear in the same windows
   Repair Targets:
   - `canon-utils/canon-runtime/src/lib.rs`
     - ensure runtime lifecycle and stage events are emitted even when rustc capture is active
   - event-log hygiene
     - separate rustc-capture-heavy traffic from canonical runtime control observability, or at minimum guarantee runtime-start / route / loop signals remain visible in the same canonical stream

## Planner Handoff
- highest-value repair targets in order:
  1. Prove the active launched runtime binary matches current source and writes to the same canonical `state/event_log/event.tlog.d` being inspected.
  2. Add a hard invariant in `EventRuntime::new` / pre-dispatch path: control-consumer count must be non-zero; emit canonical startup registration evidence with consumer names.
  3. Remove queue-local routing authority from `canon-utils/canon-route/src/policy.rs` (`dispatch_route_proposal`, `event_route_proposal`, `evaluate_route_event_dispatch`).
  4. Remove queue-local reroute authority from `canon-utils/canon-route/src/executor.rs` unless explicitly backed by semantic-state-derived mirrors.
  5. Add tests/invariants that fail when `planned_pending`, `scheduler_len`, or queue emptiness alone change route choice.
  6. Stop relying on stale ad-hoc debug logs; write runtime startup and dispatch registration into canonical artifacts.
- blockers / missing evidence:
  - no fresh canonical runtime-start or bus-trace events were present in the current event-log window, so the exact live executable path is still inferred from absence plus stale debug artifacts rather than directly confirmed from fresh runtime-start events
  - because the current canonical stream is rustc-dominated, the planner should prioritize restoring canonical runtime observability before deeper behavioral diagnosis

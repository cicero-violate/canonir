# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 10)

1. REMOVE REMAINING QUEUE-LOCAL AUTHORITY FROM LIVE `canon-loop` CONTROL SURFACES
   - Run `rg -n "pending_plan|pending_act|planned_count|decision_emitted_this_tick|last_decision_tick|scheduler_len|planned_pending" canon-utils/canon-loop` from `/workspace/ai_sandbox/canon`.
   - Read `canon-utils/canon-loop/src/context.rs`, `canon-utils/canon-loop/src/executor.rs`, `canon-utils/canon-loop/src/stage/plan.rs`, and `canon-utils/canon-loop/src/stage/act.rs` where those fields still influence control legality or readiness.
   - Patch `canon-utils/canon-loop/src/context.rs` so queue-local mirrors are not treated as canonical truth for routing / control invariants.
   - Patch `canon-utils/canon-loop/src/executor.rs` so `pending_*`, `planned_count`, and similar mirrors are bookkeeping only and do not decide control legality.
   - Patch any remaining stage-level gates in `stage/plan.rs` and `stage/act.rs` that still use queue-local state as root truth.
   - Extend regression tests so identical semantic state yields identical control outcomes even when queue-local mirrors differ.
   - Test:
     - `cargo test -p canon-loop`

2. RESTORE FRESH CANONICAL PERSISTENCE OF NON-RUSTC RUNTIME/CONTROL EVENTS
   - Read `canon-utils/canon-runtime/src/lib.rs`, especially `emit_tick`, `emit_event`, `emit_event_located`, `handle_replayed_event`, and `drain_emitted_events`, plus `canon-utils/canon-runtime/src/bin/event_runtime.rs` around the live loop.
   - Audit whether runtime control events are appended into the same segmented log head that rustc writes to, and identify any stale or alternate path.
   - Patch runtime persistence so active execution emits fresh non-rustc control events into `state/event_log/event.tlog.d`.
   - Add a smoke/integration test that emits a Tick through the live path and proves the newest segmented log window contains corresponding non-rustc runtime/control events.
   - Add a freshness invariant/report that fails when active runtime execution produces only rustc traffic in the newest canonical head.
   - Test:
     - `cargo test -p canon-runtime`

3. ENFORCE PER-CYCLE PROGRESSION AND EXACTLY-ONE-DECISION INVARIANTS AT RUNTIME
   - Read `canon-utils/canon-runtime/src/lib.rs:393-420`, `canon-utils/canon-loop/src/context.rs:168-171,270-271`, `canon-utils/canon-loop/src/executor.rs:675-720,917-922`, and the relevant route-decision emission sites.
   - Remove disabled `if false` guards blocking duplicate-decision and missing-`RouteSelected` enforcement.
   - Patch the runtime / loop path so each cycle records authoritative proof for `Tick -> RouteTick -> Decision -> RouteSelected` and discharges the cycle only when the full chain occurs.
   - Make zero-decision and multi-decision cycles explicit runtime failures.
   - Add focused tests that fail on missing `Decision`, missing `RouteSelected`, or duplicate decisions within one tick.
   - Test:
     - `cargo test -p canon-runtime`
     - `cargo test -p canon-loop`

4. ADD DURABLE EVENTBUS DELIVERY ACCOUNTING AND ELIMINATE SILENT DELIVERY GAPS
   - Read `canon-utils/canon-runtime/src/bus.rs:53-85` before patching.
   - Add receipt accounting keyed by `(event_id, consumer_name)` and expose a post-dispatch completeness assertion/report.
   - Convert `consumer.lock()` acquisition failure into explicit runtime evidence or validation failure instead of a silent skip.
   - Add execution tests comparing emitted events against per-consumer receipts and fail on any missing delivery.
   - Persist delivery-audit summaries into canonical runtime events where needed so Objective 1 can be proven from logs.
   - Test:
     - `cargo test -p canon-runtime`

5. CLOSE RUNTIME DETERMINISM, ASYNC PROPAGATION, AND HIDDEN-ROUTE PROOF OBLIGATIONS
   - Re-read `PLANS/OBJECTIVES.md`, `canon-utils/canon-route/src/decision.rs`, decision-entry callers, and async re-entry surfaces in runtime / loop.
   - Add runtime replay or snapshot-comparison checks proving identical `SemanticStateSummary` yields identical decision output and `RouteSelected` outcome.
   - Add async propagation tracing from async emission to EventBus delivery to loop observation to downstream decision effect.
   - Add a focused routing-path audit proving live `RouteSelected` emissions still originate from the intended decision boundary.
   - Test:
     - `cargo test -p canon-runtime`
     - `cargo test -p canon-route`
     - `cargo test -p canon-loop`

## BLOCKED / FOLLOW-ON

- Do not spend the first slot redoing `harness_repair.rs` semantic gating that now appears already repaired and regression-tested.
- Do not treat stale canonical heads or policy-layer unit tests as proof of runtime execution correctness.
- Keep analyst/watchdog cleanup and broader route-regression hardening behind the runtime-proof critical path unless new evidence reorders priorities.

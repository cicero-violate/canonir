# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 10)

1. ENFORCE RUNTIME LOCK CORRECTNESS (FAIL-FAST START)
    - Read `canon-utils/canon-runtime/src/bin/event_runtime.rs`.
    - Locate `acquire_lock` usage and continuation path in `main()`.
    - Patch so lock acquisition failure causes immediate exit/panic.
    - Emit `runtime_start_failed` on failure and `runtime_started` on success.
    - Verify second runtime instance cannot start.
    - Test:
      - run runtime twice → second instance must fail

2. GUARANTEE LOOP HEARTBEAT (TICK WITHOUT INPUT)

    - Read `handle_event_msg` and `emit_tick` call sites.
    - Identify dependency on external EventMsg.
    - Patch runtime loop to emit Tick on timer or loop iteration.
    - Add invariant: Tick must occur within bounded interval.
    - Verify Tick events appear with no external input.
    - Test:
      - start runtime idle → confirm Tick events in log

3. RESTORE EVENT INGRESS PATH (WATCHER/BOOTSTRAP)

    - Read watcher thread in `event_runtime.rs` (~530+).
    - Verify enqueue path into runtime queue.
    - Ensure bootstrap emits initial event.
    - Add invariant: at least one event must enter system post-start.
    - Confirm non-rustc events begin appearing in log.

4. RESTORE CONTROL-FLOW EMISSION PIPELINE (Decision → RouteSelected)
    - Run `rg -n "RouteSelected|Decision|canon_emit!" canon-utils`.
    - Trace decision output from `canon-invariant::decide` into runtime.
    - Verify conversion into `RuntimeEvent::RouteSelected`.
    - Ensure all control events use `canon_emit!(emitter; ...)`.
    - Patch missing bridges so every decision produces exactly one `RouteSelected`.
    - Verify runtime loop produces `Tick -> RouteTick -> Decision -> RouteSelected`.
    - Verify newest tlog segments contain non-rustc control events.
    - Test:
      - `cargo test -p canon-runtime`

5. PROMOTE EVENTBUS DELIVERY GAPS AND HOOK DECISIONS TO ENFORCEMENT
    - Read `canon-utils/canon-runtime/src/bus.rs` and `hooks.rs`.
    - Convert `dispatch_delivery_gap` and `dispatch_consumer_lock_failed` into invariant failures.
    - Block `HookDecision::Deny` / `Mutate` for control events.
    - Ensure runtime halts or rejects on violation.
    - Test:
      - `cargo test -p canon-runtime`

6. REMOVE REMAINING QUEUE-LOCAL AUTHORITY FROM LIVE `canon-loop` CONTROL SURFACES

    - Run `rg -n "pending_plan|pending_act|planned_count|decision_emitted_this_tick|last_decision_tick|scheduler_len|planned_pending" canon-utils/canon-loop` from `/workspace/ai_sandbox/canon`.
    - Classify each usage as control vs observational.
    - Read `canon-utils/canon-loop/src/harness_repair.rs`, `canon-utils/canon-loop/src/context.rs`, `canon-utils/canon-loop/src/executor.rs`, `canon-utils/canon-loop/src/stage/plan.rs`, and `canon-utils/canon-loop/src/stage/act.rs` to verify which queue-local fields still influence control legality or readiness.
    - Patch `canon-utils/canon-loop/src/context.rs` so queue-local mirrors are not treated as canonical truth for routing / control invariants.
    - Patch `canon-utils/canon-loop/src/executor.rs` so `pending_*`, `planned_count`, and similar mirrors are bookkeeping only and do not decide control legality.
    - Patch any remaining stage-level gates in `stage/plan.rs` and `stage/act.rs` that still use queue-local state as root truth.
    - Add regression test where queue-local mirrors diverge but semantic state remains identical.
   - Extend regression tests so identical semantic state yields identical control outcomes even when queue-local mirrors differ.
   - Test:
     - `cargo test -p canon-loop`

7. ELIMINATE NON-SEMANTIC REPLAY SUPPRESSION PATHS
    - Read `canon-utils/canon-runtime/src/bus.rs:97-236` and `canon-utils/canon-runtime/src/hooks.rs:42-145` before patching.
    - Identify all emit paths for `dispatch_delivery_gap` and `dispatch_consumer_lock_failed`.
    - Patch `canon-utils/canon-runtime/src/bus.rs` so `dispatch_delivery_gap` and `dispatch_consumer_lock_failed` become invariant failures or runtime halts for required event classes instead of continue-after-audit behavior.
    - Patch `canon-utils/canon-runtime/src/bus.rs` and `canon-utils/canon-runtime/src/hooks.rs` so protected-control `HookDecision::Deny` / `Mutate` fail closed rather than auditing and continuing.
    - Preserve audit events, but make the runtime outcome change when delivery or protected-hook invariants are violated.
    - Ensure audit events remain but cannot allow continuation.
   - Add execution tests that assert delivery gaps and protected-hook decisions fail or halt rather than only producing diagnostics.
   - Test:
     - `cargo test -p canon-runtime`

8. RESTORE FRESH CANONICAL PERSISTENCE OF NON-RUSTC RUNTIME/CONTROL EVENTS
    - Read `canon-utils/canon-runtime/src/lib.rs`, especially replay suppression/accounting surfaces and replay-side emit/discard paths.
    - Identify all `replay_suppressed_*` branches.
    - Patch replay so suppression decisions become lawful canonical state transitions or explicit invariant failures rather than conditional control branches.
    - Keep replay audit evidence visible, but remove runtime branches that suppress control flow outside semantic state.
    - Verify no control path skips emission without canonical record.
   - Add focused tests proving replay suppression cannot silently change reconstruction or downstream control behavior.
   - Test:
     - `cargo test -p canon-runtime`

9. ENFORCE PER-CYCLE PROGRESSION AND EXACTLY-ONE-DECISION INVARIANTS
    - Read `canon-utils/canon-runtime/src/lib.rs`, especially `emit_tick`, `emit_event`, `emit_event_located`, `handle_replayed_event`, and `drain_emitted_events`, plus `canon-utils/canon-runtime/src/bin/event_runtime.rs` around the live loop.
    - Trace full path from emit to writer append.
    - Audit whether runtime control events are appended into the same segmented log head that rustc writes to, and identify any stale or alternate path.
    - Patch runtime persistence so active execution emits fresh non-rustc control, delivery-audit, and replay-audit events into `state/event_log/event.tlog.d`.
    - Ensure runtime uses same writer as rustc path.
   - Add a smoke/integration test that emits a Tick through the live path and proves the newest segmented log window contains corresponding non-rustc runtime/control events.
   - Add a freshness invariant/report that fails when active runtime execution produces only rustc traffic in the newest canonical head.
   - Test:
     - `cargo test -p canon-runtime`

10. CLOSE DETERMINISM, ASYNC PROPAGATION, AND HIDDEN-ROUTE PROOFS
    - Read `canon-utils/canon-runtime/src/lib.rs:393-420`, `canon-utils/canon-loop/src/context.rs:168-172,271-272`, `canon-utils/canon-loop/src/executor.rs:674-719,917-922`, and the relevant route-decision emission sites.
    - Remove disabled `if false` guards blocking duplicate-decision and missing-`RouteSelected` enforcement.
    - Add per-cycle counters keyed by tick id.
    - Patch the runtime / loop path so each cycle records authoritative proof for `Tick -> RouteTick -> Decision -> RouteSelected` and discharges the cycle only when the full chain occurs.
    - Make zero-decision and multi-decision cycles explicit runtime failures.
    - Fail cycle immediately on invariant violation.
   - Add focused tests that fail on missing `Decision`, missing `RouteSelected`, or duplicate decisions within one tick.
   - Test:
     - `cargo test -p canon-runtime`
     - `cargo test -p canon-loop`

    - Re-read `PLANS/OBJECTIVES.md`, `canon-utils/canon-route/src/decision.rs`, `canon-utils/canon-route/src/executor.rs`, `canon-utils/canon-runtime/src/consumers/dispatch_consumer.rs`, decision-entry callers, and async re-entry surfaces in runtime / loop.
    - Add deterministic replay check for identical SemanticStateSummary.
    - Add runtime replay or snapshot-comparison checks proving identical `SemanticStateSummary` yields identical decision output and `RouteSelected` outcome.
    - Add async propagation tracing from async emission to EventBus delivery to loop observation to downstream decision effect.
    - Add a focused routing-path audit proving live `RouteSelected` emissions still originate from the intended decision boundary.
    - Assert all RouteSelected originate from decision boundary only.
   - Audit `GLOBAL_LAST_DECISION` and `LOOP_OBSERVED_SEEN_TICKS`; remove them or prove they are observational only and not hidden control truth.
   - Test:
     - `cargo test -p canon-runtime`
     - `cargo test -p canon-route`
     - `cargo test -p canon-loop`

## BLOCKED / FOLLOW-ON

- Do not treat delivery-gap audit events, hook-pre audit records, or replay-suppression diagnostics as sufficient correctness by themselves.
- Do not treat stale canonical heads or policy-layer unit tests as proof of runtime execution correctness.
- Keep analyst/watchdog cleanup and broader route-regression hardening behind the semantic-authority / enforcement / runtime-proof critical path unless new evidence reorders priorities.

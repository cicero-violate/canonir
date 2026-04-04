# PLAN: Enforce Runtime Boundaries, Then Prove Live Control

## A. Authoritative Context

### Canonical Law
- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- Canon must progress as `state -> decision -> transition -> event log`.
- Executors execute approved work; they do not invent route truth from queue-local bookkeeping.
- `scheduler_len`, `planned_pending`, `pending_plan`, `pending_act`, and similar counters are not routing authority.
- Replay, hooks, dispatch, and global caches may not introduce hidden suppression, hidden state, or non-canonical control paths.
- This planning cycle may modify only `PLAN.md` and lane plans. Do not touch `canon-utils/canon-mini-agent`.

### Current Evidence From Disk
- `PLANS/OBJECTIVES.md` requires executable proof for EventBus delivery, hook safety, per-cycle guarantees, exactly-one decision, determinism, async propagation, and no hidden routing paths.
- `VIOLATIONS.md` now reports that delivery gaps, lock failures, and protected hook decisions are observable but still not enforced as hard correctness boundaries.
- `PLANS/agent_0/diagnostics-agent_0.md` still reports fresh canonical heads as rustc-only, so live runtime/control behavior remains unproven in the newest segmented logs.
- Fresh source search shows remaining queue-local control mirrors in `canon-utils/canon-loop/src/harness_repair.rs`, `context.rs`, `executor.rs`, `stage/plan.rs`, and `stage/act.rs`.
- Fresh source search shows `canon-utils/canon-runtime/src/bus.rs` and `hooks.rs` now emit audit evidence for delivery gaps and protected-control hook decisions, but the runtime still continues afterward.
- Fresh source search shows explicit replay-suppression/accounting surfaces in `canon-utils/canon-runtime/src/lib.rs`, and additional global-state surfaces in `canon-utils/canon-route/src/executor.rs` and `canon-utils/canon-runtime/src/consumers/dispatch_consumer.rs` that must be audited before determinism can be claimed.

### Planning Consequence
- Keep semantic-state authority first.
- Promote enforcement of observed delivery gaps, protected-control hook decisions, and replay suppression ahead of generic validator work.
- Restore fresh canonical persistence and then land per-cycle runtime proof only after those boundaries fail closed instead of merely logging warnings.

## B. Ordered Root Failures

### -2. Runtime does not start correctly (lock failure allows degraded execution)
Why first:
- Diagnostics show runtime continues without lock, meaning no correctness guarantees.
- If runtime is not authoritative, all downstream validation is invalid.

Required result:
- Runtime must fail fast if lock acquisition fails.
- Add invariant: runtime must hold lock to execute.
- Emit runtime_started / runtime_active events.

### -1. Runtime loop is not driven (no heartbeat, no Tick emission)
Why second:
- Diagnostics show no control events because loop is never triggered.
- emit_tick depends on incoming events → system stalls without ingress.

Required result:
- Runtime must emit Tick independent of external events.
- Add invariant: bounded-time Tick emission.
- Ensure bootstrap injects initial event.

### 0. Event ingress path is broken (watcher/bootstrap not feeding runtime)
Why third:
- Diagnostics show only rustc events → runtime not receiving inputs.
- Without ingress, loop cannot progress even with heartbeat.

Required result:
- Verify watcher → queue → handle_event_msg path.
- Ensure at least one initial event enters system.

### 1. Control-flow emission pipeline is broken (Decision → RouteSelected not emitted)
Why fourth:
- Even if runtime runs, no control-flow events are emitted.
- Required for all downstream invariants.

### 0. Control-flow emission pipeline is broken (Decision → RouteSelected not emitted)
Why first:
- Diagnostics show zero control-flow events in logs (no Decision, RouteSelected, LoopObserved).
- All downstream invariants depend on emission existing.

Required result:
- `canon-invariant::decide` output must be emitted as `RouteSelected`.
- Runtime loop must produce `Tick -> RouteTick -> Decision -> RouteSelected`.
- ≥1 control event per cycle must exist in canonical log.

### 1. Queue-local mirrors still influence live loop control surfaces
Why first:
- Canonical law requires semantic-state authority before runtime proof.
- Diagnostics still point to `harness_repair.rs`, `context.rs`, `executor.rs`, and loop stages as live control surfaces where queue-local mirrors remain.

Required result:
- `canon-loop` control legality and readiness must derive from semantic / constraint state, not from `pending_plan`, `pending_act`, `planned_count`, or similar mirrors.
- Regression tests must prove identical semantic state yields identical control outcomes even when queue-local mirrors differ.

### 2. EventBus delivery gaps and protected-control hook decisions are observable but not enforced
Why second:
- Delivery gaps, lock failures, and protected hook decisions are now visible, but the runtime still continues after detecting them.
- Objectives 1 and 2 require no-drop and no-mutation/no-suppression as hard correctness boundaries.

Required result:
- Promote `dispatch_delivery_gap` and `dispatch_consumer_lock_failed` from audit to invariant enforcement for required event classes.
- Make protected-control `HookDecision::Deny` / `Mutate` fail closed or halt instead of auditing and continuing.

### 3. Replay suppression remains an explicit but non-semantic control path
Why third:
- Replay suppression is now logged, but still exists as conditional runtime behavior outside semantic state.
- Explicit logging is an improvement, but not sufficient for event-sourced correctness.

Required result:
- Encode replay suppression decisions into canonical state / events or eliminate those conditional paths entirely.
- Remove replay-side conditional behavior that suppresses control flow without becoming a lawful state transition.

### 4. Fresh canonical log heads still lack non-rustc runtime/control events
Why fourth:
- Diagnostics show the newest segmented log window is still rustc-only, so live runtime correctness is not provable from the canonical log.
- Enforcement work must become visible in the active segmented head, not only in stale logs or unit tests.

Required result:
- Active runtime execution must persist fresh `Tick`, `RouteTick`, decision-path, delivery-audit, and replay-audit events into `state/event_log/event.tlog.d`.
- Add a freshness invariant or smoke test proving live runtime/control events appear in the newest segmented logs.

### 5. Per-cycle `Tick -> RouteTick -> Decision -> RouteSelected` and exactly-one-decision invariants are still not enforced at runtime
Why fifth:
- Disabled guards and partial instrumentation still leave the runtime proof obligation open.
- Per-cycle proof should land after semantic authority, delivery/hook enforcement, replay enforcement, and fresh persistence are trustworthy.

Required result:
- Remove disabled guards and make missing / duplicate decision failures explicit at runtime.
- Record authoritative cycle proof surfaces rather than partial debug state.

### 6. Runtime determinism, async propagation, and hidden-route proof remain follow-on proof obligations
Why sixth:
- These remain required, but depend on lawful replay, lawful dispatch, fresh logs, and per-cycle markers first.
- Fresh source search shows additional global state surfaces that must be audited before determinism can be claimed.

Required result:
- Add runtime replay / snapshot equivalence checks, async propagation tracing, and routing-path audit tests.
- Audit and remove or justify `GLOBAL_LAST_DECISION` and `LOOP_OBSERVED_SEEN_TICKS` if they influence live control truth outside canonical state.

## C. Dependency-Ordered Work

### Phase -1 - Unify canonical event stream (tlog path)
1. Search for all tlog path definitions:
   - `rg -n "event_log|tlog|CANON_TLOG_PATH" canon-utils`
2. Identify all writers/readers:
   - runtime (`canon-runtime`)
   - rustc wrapper (`canon-rustc`)
   - supervisor / binaries
   - diagnostics scripts
3. Standardize on single path: `canon/state/event_log/event.tlog.d`
4. Patch all components to use shared env/config (`CANON_TLOG_PATH`).
5. Add fail-fast if paths diverge.
6. Add invariant: all actors must emit into same stream.
7. Test:
   - run runtime + rustc and confirm mixed actor entries in same log segment.

### Phase 0 - Restore control-flow emission pipeline
1. Run `rg -n "RouteSelected|Decision|canon_emit!" canon-utils`.
2. Trace Decision → RouteSelected path across canon-invariant → canon-route → canon-loop.
3. Ensure decision output is converted into `RuntimeEvent::RouteSelected`.
4. Ensure all control events use `canon_emit!(emitter; ...)`.
5. Verify runtime loop emits `Tick -> RouteTick -> Decision -> RouteSelected`.
6. Add invariant: ≥1 control event per cycle.
7. Test:
   - `cargo test -p canon-runtime`

### Phase 1 - Remove remaining queue-local authority from live `canon-loop` control paths
1. Run `rg -n "pending_plan|pending_act|planned_count|decision_emitted_this_tick|last_decision_tick|scheduler_len|planned_pending" canon-utils/canon-loop`.
2. Read `canon-utils/canon-loop/src/harness_repair.rs`, `context.rs`, `executor.rs`, `stage/plan.rs`, and `stage/act.rs` to verify which queue-local fields still influence control legality or readiness.
3. Patch `context.rs` so queue-local mirrors are not treated as canonical truth for routing / control invariants.
4. Patch `executor.rs` so `pending_*`, `planned_count`, and similar mirrors are bookkeeping only and do not decide control legality.
5. Patch any remaining stage-level gates in `stage/plan.rs` and `stage/act.rs` that still use queue-local state as root truth.
6. Extend regression tests so identical semantic state yields identical control outcomes even when queue-local mirrors differ.
7. Test:
   - `cargo test -p canon-loop`

### Phase 2 - Promote delivery gaps and protected-hook decisions from audit to enforcement
1. Read `canon-utils/canon-runtime/src/bus.rs:97-236` and `canon-utils/canon-runtime/src/hooks.rs:42-145` before patching.
2. Patch `bus.rs` so `dispatch_delivery_gap` and `dispatch_consumer_lock_failed` become invariant failures or runtime halts for required event classes instead of continue-after-audit behavior.
3. Patch `bus.rs` and `hooks.rs` so protected-control `HookDecision::Deny` / `Mutate` fail closed rather than auditing and continuing.
4. Preserve audit events, but make the runtime outcome change when delivery or protected-hook invariants are violated.
5. Add execution tests that assert delivery gaps and protected-hook decisions fail or halt rather than only producing diagnostics.
6. Test:
   - `cargo test -p canon-runtime`

### Phase 3 - Eliminate non-semantic replay suppression paths while keeping replay accounting visible
1. Read `canon-utils/canon-runtime/src/lib.rs`, especially replay suppression/accounting surfaces and replay-side emit/discard paths.
2. Patch replay so suppression decisions become lawful canonical state transitions or explicit invariant failures rather than conditional control branches.
3. Keep replay audit evidence visible, but remove runtime branches that suppress control flow outside semantic state.
4. Add focused tests proving replay suppression cannot silently change reconstruction or downstream control behavior.
5. Test:
   - `cargo test -p canon-runtime`

### Phase 4 - Restore fresh canonical persistence of non-rustc runtime/control events
1. Read `canon-utils/canon-runtime/src/lib.rs`, especially `emit_tick`, `emit_event`, `emit_event_located`, `handle_replayed_event`, and `drain_emitted_events`, plus `canon-utils/canon-runtime/src/bin/event_runtime.rs` around the live loop.
2. Audit whether runtime control events are appended into the same segmented log head that rustc writes to, and identify any stale or alternate path.
3. Patch runtime persistence so active execution emits fresh non-rustc control, delivery-audit, and replay-audit events into `state/event_log/event.tlog.d`.
4. Add a smoke/integration test that emits a Tick through the live path and proves the newest segmented log window contains corresponding non-rustc runtime/control events.
5. Add a freshness invariant/report that fails when active runtime execution produces only rustc traffic in the newest canonical head.
6. Test:
   - `cargo test -p canon-runtime`

### Phase 5 - Enforce per-cycle progression and exactly-one-decision invariants at runtime
1. Read `canon-utils/canon-runtime/src/lib.rs:393-420`, `canon-utils/canon-loop/src/context.rs:168-172,271-272`, `canon-utils/canon-loop/src/executor.rs:674-719,917-922`, and relevant route-decision emission sites.
2. Remove disabled `if false` guards blocking duplicate-decision and missing-`RouteSelected` enforcement.
3. Patch the runtime / loop path so each cycle records authoritative proof for `Tick -> RouteTick -> Decision -> RouteSelected` and discharges the cycle only when the full chain occurs.
4. Make zero-decision and multi-decision cycles explicit runtime failures.
5. Add focused tests that fail on missing `Decision`, missing `RouteSelected`, or duplicate decisions within one tick.
6. Test:
   - `cargo test -p canon-runtime`
   - `cargo test -p canon-loop`

### Phase 6 - Close runtime determinism, async propagation, and hidden-route proof obligations
1. Re-read `PLANS/OBJECTIVES.md`, `canon-utils/canon-route/src/decision.rs`, `canon-utils/canon-route/src/executor.rs`, `canon-utils/canon-runtime/src/consumers/dispatch_consumer.rs`, decision-entry callers, and async re-entry surfaces in runtime / loop.
2. Add runtime replay or snapshot-comparison checks proving identical `SemanticStateSummary` yields identical decision output and `RouteSelected` outcome.
3. Add async propagation tracing from async emission to EventBus delivery to loop observation to downstream decision effect.
4. Add a focused routing-path audit proving live `RouteSelected` emissions still originate from the intended decision boundary.
5. Audit `GLOBAL_LAST_DECISION` and `LOOP_OBSERVED_SEEN_TICKS`; remove them or prove they are observational only and not hidden control truth.
6. Test:
   - `cargo test -p canon-runtime`
   - `cargo test -p canon-route`
   - `cargo test -p canon-loop`

## D. Ready-Work Window

### `executor_pool` READY NOW
1. Enforce runtime lock correctness (fail-fast start boundary).
    1. Read `canon-utils/canon-runtime/src/bin/event_runtime.rs`.
    2. Locate `acquire_lock` and continuation path in `main()`.
    3. Patch so failure to acquire lock causes immediate exit or panic.
    4. Emit `runtime_start_failed` on failure and `runtime_started` on success.
    5. Verify second runtime instance cannot start.

2. Guarantee loop heartbeat (Tick emission without input dependency).
    1. Read `handle_event_msg` and `emit_tick` call sites.
    2. Identify dependency on external EventMsg.
    3. Patch runtime loop to emit Tick on timer or loop iteration.
    4. Add invariant: Tick must occur within bounded interval.
    5. Verify Tick events appear with zero external input.

3. Restore event ingress path (watcher/bootstrap → runtime queue).
    1. Read watcher thread in `event_runtime.rs` (~530+).
    2. Verify enqueue path into runtime queue.
    3. Ensure bootstrap emits initial event.
    4. Add invariant: at least one event must enter system post-start.
    5. Confirm non-rustc events begin appearing in log.

4. Restore control-flow emission pipeline (Decision → RouteSelected → EventBus).
    1. Run `rg -n "RouteSelected|Decision|canon_emit!" canon-utils`.
    2. Trace decision output from `canon-invariant::decide` into runtime.
    3. Bridge decision output to `RuntimeEvent::RouteSelected`.
    4. Ensure all control events use `canon_emit!(emitter; ...)`.
    5. Verify `Tick -> RouteTick -> Decision -> RouteSelected` appears in log.

5. Promote EventBus delivery gaps and protected-hook decisions to enforcement.
    1. Read `canon-runtime/src/bus.rs` and `hooks.rs`.
    2. Convert delivery gaps into invariant failures (halt/reject).
    3. Block `HookDecision::Deny/Mutate` for control events.
    4. Ensure runtime cannot continue after violation.

6. Remove remaining queue-local authority from `canon-loop` control surfaces.
    1. Run rg for `pending_*`, `planned_*`, `scheduler_len`.
    2. Classify control vs observational uses.
    3. Patch control uses to derive from SemanticStateSummary.
    4. Add regression test for semantic equivalence with divergent queues.

7. Eliminate replay suppression as a non-semantic control path.
    1. Identify `replay_suppressed_*` branches.
    2. Replace with canonical state transition or invariant failure.
    3. Ensure no control path skips emission.

8. Restore fresh canonical persistence of runtime/control events.
    1. Trace emit path to tlog writer.
    2. Ensure runtime shares canonical writer with rustc.
    3. Add smoke test proving newest log contains runtime events.

9. Enforce per-cycle invariants (Tick → Decision → RouteSelected, exactly-one decision).
    1. Remove disabled guards.
    2. Add cycle_id tracking.
    3. Fail on missing or duplicate decisions.

10. Close determinism, async propagation, and hidden-route proofs.
    1. Add replay equivalence checks.
    2. Trace async event propagation to decision impact.
    3. Audit and eliminate hidden routing paths.

## E. Blocked / Follow-On
- Do not treat delivery-gap audit events, hook-pre audit records, or replay-suppression diagnostics as sufficient correctness by themselves.
- Do not treat stale canonical heads or policy-layer unit tests as proof of runtime execution correctness.
- Keep analyst/watchdog cleanup behind the semantic-authority / enforcement / runtime-proof critical path unless new evidence reorders priorities.
### Phase -3 - Enforce runtime lock correctness
1. Read `canon-utils/canon-runtime/src/bin/event_runtime.rs`.
2. Locate `acquire_lock` usage and `main()` continuation path.
3. Patch so lock failure causes immediate exit/panic.
4. Add invariant event: runtime_start_failed if lock not acquired.
5. Add runtime_started event when lock acquired.
6. Test:
   - run runtime twice → second instance must fail.

### Phase -2 - Guarantee loop heartbeat (Tick emission without input)
1. Read `handle_event_msg` and `emit_tick` call sites.
2. Identify dependency on external EventMsg.
3. Patch runtime loop to emit Tick on timer or loop iteration.
4. Add invariant: Tick must occur within bounded interval.
5. Ensure Tick flows into RouteTick → Decision pipeline.
6. Test:
   - start runtime with no inputs → confirm Tick events in log.

### Phase -1 - Restore event ingress path (watcher/bootstrap)
1. Read watcher thread in `event_runtime.rs` (~530+).
2. Verify events are enqueued into runtime queue.
3. Ensure bootstrap emits initial event.
4. Add invariant: system must receive at least one event after startup.
5. Test:
   - start runtime → confirm first non-rustc event appears.

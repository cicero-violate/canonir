# PLAN: Restore Live Runtime Proof After Semantic-State Authority Cleanup

## A. Authoritative Context

### Canonical Law
- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- Canon must progress as `state -> decision -> transition -> event log`.
- Executors execute approved work; they do not invent route truth from queue-local bookkeeping.
- `scheduler_len`, `planned_pending`, `pending_plan`, `pending_act`, and similar counters are not routing authority.
- This planning cycle may modify only `PLAN.md` and lane plans. Do not touch `canon-utils/canon-mini-agent`.

### Current Evidence From Disk
- `PLANS/SPEC.md` defines Canon as an event-sourced judgment system and rejects scheduler-first orchestration.
- `PLANS/OBJECTIVES.md` requires executable proof for EventBus delivery, per-cycle guarantees, exactly-one decision, determinism, async propagation, and no hidden routing paths.
- `VIOLATIONS.md` now additionally records that the runtime loop is not exercised under execution, exactly-one-decision is not enforced at runtime, determinism is not proven at runtime, and EventBus may still allow silent delivery gaps.
- `PLANS/agent_0/diagnostics-agent_0.md` reports fresh canonical heads remain rustc-only and therefore do not currently prove live runtime/control behavior.
- Fresh source search shows `canon-utils/canon-loop/src/harness_repair.rs:112,117` already moved `verifier_ready` / `needs_replan` to semantic helpers and includes queue-noise regression tests, so that repair surface is no longer the main blocker.
- Fresh source search still shows queue-local control mirrors and proof gaps in `canon-utils/canon-loop/src/context.rs:149-182,270-271`, `canon-utils/canon-loop/src/executor.rs:497-498,675-720,920-922`, `canon-utils/canon-loop/src/stage/plan.rs:891`, `canon-utils/canon-loop/src/stage/act.rs:51-52,277-333,388`, `canon-utils/canon-runtime/src/lib.rs:393-420,636`, and `canon-utils/canon-runtime/src/bus.rs:65`.

### Planning Consequence
- Narrow the first task from already-fixed `harness_repair.rs` semantics to the remaining queue-local authority still present in `context.rs`, `executor.rs`, and loop stages.
- Elevate fresh canonical persistence of non-rustc runtime/control events ahead of generic validator work, because current log heads cannot prove execution.
- Keep per-cycle exactly-one-decision enforcement immediately after log freshness is restored, then land delivery accounting and runtime determinism proof.

## B. Ordered Root Failures

### 1. Remaining queue-local mirrors still influence live loop control surfaces
Why first:
- `harness_repair.rs` already moved to semantic helpers, but diagnostics still cite `context.rs`, `executor.rs`, and stage files as places where queue-local mirrors remain in control paths.
- Until those remaining surfaces are demoted to bookkeeping-only or removed from control truth, runtime proof risks validating the wrong authority.

Required result:
- `canon-loop` control decisions must derive from semantic / constraint state, not from `pending_plan`, `pending_act`, `planned_count`, or similar mirrors.
- Existing semantic helper tests must be extended to cover the remaining `context.rs`, `executor.rs`, and stage-level surfaces.

### 2. Fresh canonical log heads still lack non-rustc runtime/control events
Why second:
- Diagnostics show the newest segmented log window is rustc-only, so runtime correctness is not provable from the canonical log.
- Objectives require proof under execution, not only source inspection or stale logs.

Required result:
- Active runtime execution must persist fresh `Tick`, `RouteTick`, decision-path, and related non-rustc control events into the same canonical segmented log head used by rustc.
- Add a freshness invariant or smoke test proving live runtime/control events appear in the newest segmented logs.

### 3. Per-cycle `Tick -> RouteTick -> Decision -> RouteSelected` and exactly-one-decision invariants are still not enforced at runtime
Why third:
- Diagnostics show disabled guards at `canon-loop/src/executor.rs:675-720` and partial instrumentation in `context.rs` that is not closing the proof obligation.
- The runtime currently checks only that some events were observed after `emit_tick()`, which is weaker than the contract.

Required result:
- Remove disabled `if false` guards and enforce missing / duplicate decision failures at runtime.
- Make cycle markers and decision counters authoritative proof surfaces, not partial debug state.

### 4. EventBus delivery completeness remains unproven
Why fourth:
- `bus.rs` still allows lock acquisition to skip delivery silently and lacks durable per-consumer receipt accounting.
- Objective 1 cannot be proven from execution evidence until delivery completeness is explicit.

Required result:
- Track delivery receipts keyed by `(event_id, consumer_name)`.
- Convert lock-acquisition failures into explicit evidence or failure and add completeness assertions/tests.

### 5. Runtime determinism, async propagation, and hidden-route proof remain unclosed
Why fifth:
- Current evidence still does not prove runtime determinism from identical semantic state, async re-entry into the loop, or single routing-path authority under live execution.
- These are follow-on proof obligations once live runtime logs, cycle markers, and delivery accounting are trustworthy.

Required result:
- Add runtime replay or snapshot equivalence checks, async propagation tracing, and routing-path audit tests.

### 6. Analyst/watchdog noise remains follow-on readability work
Why sixth:
- This still matters, but the new evidence shows the more urgent blockers are stale log freshness and unenforced runtime invariants.

Required result:
- Keep analyst/watchdog cleanup behind the runtime-proof critical path unless fresh failures re-elevate it.

## C. Dependency-Ordered Work

### Phase 1 - Remove remaining queue-local authority from live `canon-loop` control paths
1. Run `rg -n "pending_plan|pending_act|planned_count|decision_emitted_this_tick|last_decision_tick|scheduler_len|planned_pending" canon-utils/canon-loop`.
2. Read `canon-utils/canon-loop/src/context.rs`, `canon-utils/canon-loop/src/executor.rs`, `canon-utils/canon-loop/src/stage/plan.rs`, and `canon-utils/canon-loop/src/stage/act.rs` where those fields still influence control or readiness.
3. Patch `canon-utils/canon-loop/src/context.rs` so queue-local mirrors are not treated as canonical truth for routing / control invariants.
4. Patch `canon-utils/canon-loop/src/executor.rs` so `pending_*`, `planned_count`, and similar mirrors are bookkeeping only and do not decide control legality.
5. Patch any remaining stage-level control gates in `stage/plan.rs` and `stage/act.rs` that still use queue-local state as root truth.
6. Extend regression tests so identical semantic state yields identical control outcomes even when queue-local mirrors differ.
7. Test:
   - `cargo test -p canon-loop`

### Phase 2 - Restore fresh canonical persistence of non-rustc runtime/control events
1. Read `canon-utils/canon-runtime/src/lib.rs`, especially `emit_tick`, `emit_event`, `emit_event_located`, `handle_replayed_event`, and `drain_emitted_events`, plus `canon-utils/canon-runtime/src/bin/event_runtime.rs` around the live loop.
2. Audit whether runtime control events are appended into the same segmented log head that rustc writes to, and identify any stale or alternate path.
3. Patch runtime persistence so active execution emits fresh non-rustc control events into `state/event_log/event.tlog.d`.
4. Add a smoke/integration test that emits a Tick through the live path and proves the newest segmented log window contains corresponding non-rustc runtime/control events.
5. Add a freshness invariant/report that fails when active runtime execution produces only rustc traffic in the newest canonical head.
6. Test:
   - `cargo test -p canon-runtime`

### Phase 3 - Enforce per-cycle progression and exactly-one-decision invariants at runtime
1. Read `canon-utils/canon-runtime/src/lib.rs:393-420`, `canon-utils/canon-loop/src/context.rs:168-171,270-271`, `canon-utils/canon-loop/src/executor.rs:675-720,917-922`, and relevant route-decision emission sites.
2. Remove disabled `if false` guards blocking duplicate-decision and missing-`RouteSelected` enforcement.
3. Patch the runtime / loop path so each cycle records authoritative proof for `Tick -> RouteTick -> Decision -> RouteSelected` and discharges the cycle only when the full chain occurs.
4. Make zero-decision and multi-decision cycles explicit runtime failures.
5. Add targeted tests that fail on missing `Decision`, missing `RouteSelected`, or duplicate decisions within one tick.
6. Test:
   - `cargo test -p canon-runtime`
   - `cargo test -p canon-loop`

### Phase 4 - Add durable EventBus delivery accounting and no-silent-skip enforcement
1. Read `canon-utils/canon-runtime/src/bus.rs:53-85` before patching.
2. Add receipt accounting keyed by `(event_id, consumer_name)` and expose a post-dispatch completeness assertion/report.
3. Convert `consumer.lock()` acquisition failure into explicit runtime evidence or validation failure instead of a silent skip.
4. Add execution tests comparing emitted events against per-consumer receipts and fail on any missing delivery.
5. Persist delivery-audit summaries into canonical runtime events where needed so Objective 1 can be proven from logs.
6. Test:
   - `cargo test -p canon-runtime`

### Phase 5 - Close runtime determinism, async propagation, and hidden-route proof obligations
1. Re-read `PLANS/OBJECTIVES.md`, `canon-utils/canon-route/src/decision.rs`, decision-entry callers, and async re-entry surfaces in runtime / loop.
2. Add runtime replay or snapshot-comparison checks proving identical `SemanticStateSummary` yields identical decision output and `RouteSelected` outcome.
3. Add async propagation tracing from async emission to EventBus delivery to loop observation to downstream decision effect.
4. Add a focused routing-path audit proving live `RouteSelected` emissions still originate from the intended decision boundary.
5. Test:
   - `cargo test -p canon-runtime`
   - `cargo test -p canon-route`
   - `cargo test -p canon-loop`

### Phase 6 - Keep analyst/watchdog cleanup and route-regression hardening as follow-on work
1. Re-evaluate `canon-utils/canon-runtime/src/consumers/analyst_consumer.rs`, `canon-utils/canon-runtime/src/consumers/watchdog_consumer.rs`, and route-regression tests only after Phases 1-5 stabilize.
2. Elevate these repairs only if fresh failures show they block runtime-proof completion.

## D. Ready-Work Window

### `executor_pool` READY NOW
1. Remove remaining queue-local authority from `canon-loop` control surfaces in `context.rs`, `executor.rs`, and loop stages.
2. Restore fresh canonical persistence of non-rustc runtime/control events in `canon-runtime` so execution is observable in the newest segmented log head.
3. Enforce per-cycle `Tick -> RouteTick -> Decision -> RouteSelected` and exactly-one-decision invariants at runtime.
4. Add durable EventBus delivery accounting and eliminate silent delivery gaps.
5. Close runtime determinism, async propagation, and hidden-route proof obligations after Tasks 1-4 land.

## E. Blocked / Follow-On
- Do not spend the first slot redoing `harness_repair.rs` semantic gating that now appears already repaired and regression-tested.
- Do not treat stale canonical heads or policy-layer unit tests as proof of runtime execution correctness.
- Do not elevate analyst/watchdog cleanup ahead of the live runtime-proof blockers unless new evidence reorders the critical path.

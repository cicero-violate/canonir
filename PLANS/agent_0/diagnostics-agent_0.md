# Diagnostics Report

## Inputs Scanned
Event log segments reviewed:
  - Fresh head confirmed at `state/event_log/event.tlog.d/00000000000000020602.log` (`2026-04-04T05:04:11.974517`, size `5193`).
  - Latest sampled entries in that head are rustc-only: `code`, `rustc_capture_started`, `rustc_graph_artifact_written`, `rustc_capture_completed`.
  - Structured scan of the newest 160 canonical segments found `non_rustc_count=0`; top actor/kind pairs were rustc-only: `('rustc','code')=802`, `('rustc','rustc_capture_started')=177`, `('rustc','rustc_graph_artifact_written')=102`, `('rustc','rustc_capture_completed')=102`, `('rustc','rustc_capture_failed')=75`.
  - Backward scan across 2780 logs found the last non-rustc entry at `00000000000000006352.log` (`2026-04-02T21:56:50.986796`): `actor=event-runtime`, `kind=runtime_started`.
  - Window around the last non-rustc runtime activity showed older non-rustc traffic only: `('supervisor','tick')=56`, `('event-runtime','error_occurred')=32`, `('event-runtime','llm')=2`, `('observe','loop_observed')=2`, `('writer','code')=2`.
- Violations reviewed:
  - `VIOLATIONS.md:33-45` flags missing cycle tracking for `Tick -> RouteTick -> Decision -> RouteSelected` and exactly-one decision validation.
  - `VIOLATIONS.md:71-91` flags runtime loop not exercised under execution and exactly-one decision invariant not enforced.
  - `VIOLATIONS.md:93-102,125-135` flags runtime determinism as unproven and the deterministic decision invariant as violated.
- Source areas reviewed:
  - `canon-utils/canon-loop/src/harness_repair.rs:94-104`
  - `canon-utils/canon-loop/src/context.rs:83-93,149-182,270-271`
  - `canon-utils/canon-loop/src/executor.rs` grep hits around `pending_act` / `planned_count`, disabled exactly-once guards near `675-720`, and `RouteTick` emission near `917-922`
  - `canon-utils/canon-route/src/executor.rs` semantic-only cleanup comments
  - `canon-utils/canon-runtime/src/lib.rs:393-420,434-443`
  - `canon-utils/canon-runtime/src/bus.rs:53-85`
  - `PLANS/OBJECTIVES.md:14-29,32-45,49-60`
- Commands run:
  - Multiple structured python analyses over canonical event-log segments, printable payload extraction, actor/kind counting, backwards freshness scan, and direct source-file line extraction.

## Ranked Failures
1. Impact: high
   Signal: Queue-local loop state still drives routing/readiness decisions in places that should derive from semantic state only.
   Evidence:
   - `canon-utils/canon-loop/src/harness_repair.rs:98` sets `verifier_ready: ctx.pending_act.is_none() && ctx.pending_plan.is_none()`.
   - `canon-utils/canon-loop/src/harness_repair.rs:103` sets `needs_replan: ctx.consecutive_invalid_plan_batches > 0 || ctx.pending_plan.is_none()`.
   - `canon-utils/canon-loop/src/context.rs:84-87` derives canonical constraint state with `let has_plan = self.pending_plan.is_some() || semantic_goal_exists;`.
   - `canon-utils/canon-loop/src/context.rs:125,152,158` stores `pending_plan`, `last_invalid_plan_planned_count`, and `pending_act` as loop-local state.
   - `canon-utils/canon-loop/src/executor.rs:497-498` clears `self.ctx.pending_act`; `canon-utils/canon-loop/src/executor.rs:545` restores `last_invalid_plan_planned_count` from error context.
   - Canonical law says `SemanticStateSummary` is the single source of truth for routing/control flow, and queue-local mirrors are not authoritative unless proven derived mirrors.
   Repair Targets:
   - `canon-utils/canon-loop/src/harness_repair.rs`: remove `pending_plan` / `pending_act` authority from `verifier_ready` and `needs_replan`; derive these gates from semantic-state / constraint-state facts only.
   - `canon-utils/canon-loop/src/context.rs`: stop treating `pending_plan.is_some()` as canonical `has_plan` truth unless it is explicitly proven as a derived mirror of `SemanticStateSummary`.
   - `canon-utils/canon-loop/src/executor.rs`: demote `pending_*` and `planned_count` fields to observational/cache status only, or remove them from control decisions entirely.
   - Add a regression test proving routing decisions are invariant under changes to queue-local mirrors when semantic state is unchanged.

2. Impact: high
   Signal: Canonical runtime/control-flow evidence is stale or missing from the current event-log head, so execution correctness is not presently provable from the canonical log.
   Evidence:
   - Freshest canonical head is `00000000000000020602.log` at `2026-04-04T05:04:11.974517`, but sampled entries are rustc-only.
   - Structured scan of the newest 160 segments found `non_rustc_count=0`.
   - Backward scan across 2780 logs found the last non-rustc event at `00000000000000006352.log` on `2026-04-02T21:56:50`.
   - This means current canonical logs do not show fresh `event-runtime`, `supervisor`, `observe`, `route`, or other live control events even while rustc traffic continues.
   - `PLANS/OBJECTIVES.md:14-29` requires EventBus/runtime integrity to be proven under execution; stale runtime evidence blocks that proof.
   Repair Targets:
   - `canon-utils/canon-runtime/src/lib.rs`: audit `emit_event`, `emit_event_located`, `emit_tick`, `handle_replayed_event`, and `drain_emitted_events` so non-rustc runtime events are persisted into the same canonical segmented log head that rustc uses.
   - Add an explicit freshness invariant: during active runtime execution, the latest canonical log window must contain non-rustc control events, not only rustc artifacts.
   - Add a runtime smoke/integration test that emits a Tick and proves the newest segmented logs contain corresponding non-rustc runtime/control events.
   - Confirm no alternate/stale event-stream path is receiving runtime events while rustc writes to `state/event_log/event.tlog.d`.

3. Impact: high
   Signal: Per-cycle control-flow guarantees are not enforced at the runtime boundary; current checks are weaker than the contract.
   Evidence:
   - `PLANS/OBJECTIVES.md:49-60` requires each loop cycle to produce `Tick -> RouteTick -> Decision -> RouteSelected`.
   - `VIOLATIONS.md:33-45` states there is no cycle-level tracking and no exactly-one decision validation.
   - `canon-utils/canon-runtime/src/lib.rs:393-401` only fail-fast checks that `observed_events` is non-empty after `emit_tick()`, which does not prove `RouteTick`, `Decision`, or `RouteSelected` occurred.
   - `canon-utils/canon-loop/src/context.rs:168-171,270-271` contains `last_decision_tick` and `decision_emitted_this_tick`, but fresh `VIOLATIONS.md` still reports the exactly-one-decision invariant as unenforced, so these fields are not closing the proof obligation.
   - `canon-utils/canon-loop/src/executor.rs:675-720` keeps duplicate-decision and missing-`RouteSelected` guards behind `if false`, disabling those invariant checks at runtime.
   - `canon-utils/canon-loop/src/executor.rs:917-922` emits `RouteTick` on `Tick`, but the fresh canonical head still contains no non-rustc events proving the cycle progressed through decision and route emission.
   - Fresh canonical head sampling found no visible current-cycle non-rustc control events to validate the contract.
   Repair Targets:
   - `canon-utils/canon-runtime/src/lib.rs`: add per-cycle control markers/counters keyed by tick and discharge them only when the full required chain occurs.
   - `canon-utils/canon-loop/src/context.rs`: make `last_decision_tick` / `decision_emitted_this_tick` authoritative runtime-proof fields or remove them as misleading partial instrumentation.
   - `canon-utils/canon-loop/src/executor.rs`: remove the `if false` guards and enforce duplicate-decision / missing-`RouteSelected` invariants at runtime.
   - `canon-utils/canon-route` / `canon-loop`: emit explicit cycle-local decision / route markers that can be validated in logs and tests.
   - Add an invariant test that fails on any cycle missing `Decision` or `RouteSelected`, and on any cycle producing multiple decisions.

4. Impact: medium
   Signal: EventBus delivery completeness remains only partially evidenced; dispatch does not yet prove “all registered consumers received the event”.
   Evidence:
   - `PLANS/OBJECTIVES.md:14-29` requires all emitted events to reach all registered consumers with no silent drops.
   - `canon-utils/canon-runtime/src/bus.rs:53-85` iterates consumers and increments `delivered` after each `on_event`, but there is no durable per-consumer receipt ledger or emitted-vs-received reconciliation.
   - `canon-utils/canon-runtime/src/bus.rs:72-74` forwards `EventOutcome::Error` through `emit_with_parents`, but current evidence does not establish a full receipt audit across all consumers and all events.
   Repair Targets:
   - `canon-utils/canon-runtime/src/bus.rs`: add delivery accounting keyed by `(event_id, consumer_name)` and expose a post-dispatch completeness assertion/report.
   - Add an execution test that compares emitted events against per-consumer receipts and fails on any missing delivery.
   - Persist delivery audit summaries into canonical runtime events so Objective 1 can be proven from the event log itself.

5. Impact: medium
   Signal: Deterministic decision behavior is still reported as violated/unverified, and current canonical evidence does not prove otherwise.
   Evidence:
   - `VIOLATIONS.md:47-57` marks deterministic decision behavior unverified.
   - `VIOLATIONS.md:71-81` marks deterministic decision invariant violated and calls for enforcement at the decision boundary.
   - Fresh canonical head lacks current non-rustc decision-path evidence, so deterministic behavior under active execution is not demonstrated.
   Repair Targets:
   - `canon-utils/canon-route/src/decision.rs` and decision-entry callers: add deterministic output assertions and stable audit payloads for each evaluated decision.
   - Add replay/re-run tests demonstrating identical semantic-state inputs produce identical decision outputs and identical `RouteSelected` emissions.
   - Record decision audits in canonical runtime events so determinism can be checked from logs, not inferred from source comments.

## Planner Handoff
- Highest-value repair targets in order:
  1. Remove queue-local authority from `canon-loop` control decisions (`harness_repair.rs`, `context.rs`, `executor.rs`) and make semantic-state / constraint-state the only routing truth.
  2. Restore or prove fresh canonical persistence of non-rustc runtime/control events in `state/event_log/event.tlog.d` so execution correctness is observable now, not only in stale logs from `2026-04-02`.
  3. Enforce per-cycle `Tick -> RouteTick -> Decision -> RouteSelected` and exactly-one-decision invariants at runtime, not just via ad-hoc debug expectations.
  4. Add durable EventBus delivery accounting to prove Objective 1 from execution evidence.
  5. Add deterministic decision audit + replay tests at the decision boundary.
- Blockers / missing evidence:
  - Current canonical log head is dominated by rustc traffic, so runtime/control-flow correctness cannot be proven from fresh logs until non-rustc persistence is restored or explicitly validated.
  - No current-cycle canonical evidence yet proves that every `RouteSelected` still originates from the intended decision boundary under live execution.

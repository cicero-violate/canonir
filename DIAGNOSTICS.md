# Diagnostics Report

## Inputs Scanned
- event log segments reviewed
  - canonical source: `state/event_log/event.tlog.d`
  - latest inventory observed: `2159` `.log`, `2159` `.idx`, `2159` `.time`
  - recent canonical segments reviewed through `00000000000000012207.log`
  - recent signal scan showed `tick=1194`, `decision=3`, `capabilityfailed=48`, `capabilitycompleted=24`, `invariant violation=282`, `emit_child(=48)`
- violations reviewed
  - `VIOLATIONS.md`
- source areas reviewed
  - `canon-utils/canon-route/src/executor.rs`
  - `canon-utils/canon-route/src/decision.rs`
  - `canon-utils/canon-route/src/helpers.rs`
  - `canon-utils/canon-route/src/policy.rs`
  - `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs`
  - `canon-utils/canon-runtime/src/lib.rs`
  - `canon-utils/canon-runtime/src/bin/event_runtime.rs`
- extra runtime evidence reviewed
  - uploaded runtime trace showing fresh supervisor/runtime execution and route panic

## Ranked Failures
1. Impact: high
   Signal: Route executor panics before a usable control loop because `RouteSelected` emission requires `decision_trace`, but the visible emit path reaches `RouteSelected` without a proven prior trace assignment.
   Evidence:
   - Uploaded runtime trace shows panic at `canon-utils/canon-route/src/executor.rs:638`: `RouteSelected emitted without preceding decision_trace` during `LoopObserved` handling.
   - `canon-utils/canon-route/src/executor.rs:632-639`
     - `fn emit_route_selected_from_decision(...)`
     - `if self.last_decision_trace_id.is_none() { panic!("RouteSelected emitted without preceding decision_trace"); }`
   - `canon-utils/canon-route/src/executor.rs:878`
     - `self.emit_route_selected_from_decision(&decision, "".to_string());`
   - The extracted `emit_decision` tail does not show a prior assignment to `self.last_decision_trace_id` before line 878.
   Repair Targets:
   - `canon-utils/canon-route/src/executor.rs::emit_decision`
   - `canon-utils/canon-route/src/executor.rs::emit_route_selected_from_decision`
   - Establish and persist `decision_trace` before any `RouteSelected` emission.
   - Audit all writes/clears of `last_decision_trace_id` and enforce strict ordering.

2. Impact: high
   Signal: Decision authority is still implicit and under-specified rather than explicitly wired from `SemanticStateSummary`.
   Evidence:
   - `canon-utils/canon-route/src/decision.rs:19-27`
     - `decide_from_json(ctx: &RouteContext, _model_json: &str, prompt: String, _controller: &mut RouteController)`
     - decision derives from `ctx.semantic_summary.validation_blocked_by_preconditions` and `ctx.semantic_summary.compiler_repair_required`
   - `canon-utils/canon-route/src/executor.rs:848`
     - `let mut decision = decide_from_json(&self.ctx, "", prompt.clone(), &mut self.controller)`
   - `emit_decision` ignores `_model_json` name-wise and does not pass an explicit `SemanticStateSummary` value into the decision interface.
   - `prompt` and controller state still remain side inputs to decision construction.
   Repair Targets:
   - `canon-utils/canon-route/src/decision.rs`
   - `canon-utils/canon-route/src/executor.rs::emit_decision`
   - Replace `decide_from_json(...)` with an explicit semantic entrypoint, e.g. `decide_from_semantic_state(summary: &SemanticStateSummary, ...)`.
   - Make the decision input contract explicit and minimal.

3. Impact: high
   Signal: Residual non-semantic routing surfaces still exist and preserve alternate authority paths.
   Evidence:
   - `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs:28-36`
     - `evaluate_model_output(&mut self, model_json: &str, signals: &RuntimeSignals)`
     - `parse_route_selection(model_json, ...)`
     - no visible `SemanticStateSummary` input
   - `canon-utils/canon-route/src/helpers.rs:40-91`
     - `request_route_via_llm_call(...)`
     - issues `llm.call`
     - waits on `RuntimeEvent::CapabilityCompleted` / `RuntimeEvent::CapabilityFailed`
     - no visible `SemanticStateSummary` input
   Repair Targets:
   - `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs::evaluate_model_output`
   - `canon-utils/canon-route/src/helpers.rs::request_route_via_llm_call`
   - Remove, hard-disable, or strictly demote these paths so they cannot act as routing authority.

4. Impact: high
   Signal: The runtime still does not exhibit a healthy semantic control loop in the canonical event log.
   Evidence:
   - recent canonical event-log scan: `tick=1194`, `decision=3`, `route_selected_per_tick=0.0`
   - uploaded runtime trace shows `LoopObserved` handling, then route executor panic, then fatal-halt/append-guard churn rather than stable control progression.
   - uploaded runtime trace also shows `LoopObserved` appends rejected for missing parent IDs after fatal halt, indicating runtime progress is breaking before stable replayable control state is established.
   Repair Targets:
   - `canon-utils/canon-route/src/executor.rs`
   - `canon-utils/canon-runtime/src/lib.rs`
   - `canon-utils/canon-runtime/src/bin/event_runtime.rs`
   - After fixing the panic, re-verify `RouteTick -> decision -> RouteSelected -> successor` in the canonical tlog.

5. Impact: medium
   Signal: EventBus/runtime no-drop/no-mutation guarantees remain unverified under real runtime stress.
   Evidence:
   - verifier summary leaves `event bus and hooks preserve strict no-drop/no-mutation guarantees at runtime` unverified
   - uploaded runtime trace shows startup debug rejection for `missing_parent_ids` and `LoopObserved` append blocking under fatal halt / causal-chain violation.
   Repair Targets:
   - `canon-utils/canon-runtime/src/lib.rs`
   - `canon-utils/canon-runtime/src/bin/event_runtime.rs`
   - `canon-utils/canon-runtime/src/bus.rs`
   - ensure non-root emitted events always carry valid parent chains and are not silently dropped or mutated

6. Impact: medium
   Signal: Queue/local mirror influence is no longer the strongest proven blocker, but it is still not fully ruled out.
   Evidence:
   - focused executable windows no longer directly prove a live `scheduler_len == 0` routing condition
   - verifier summary still treats routing authority as undefined rather than fully semantic
   - route/context/policy code still requires a broader executable audit after the panic and authority path are fixed
   Repair Targets:
   - `canon-utils/canon-route/src/context.rs`
   - `canon-utils/canon-route/src/policy.rs`
   - `canon-utils/canon-route/src/executor.rs`
   - complete post-panic audit for residual queue-derived routing conditions

## Planner Handoff
- ordered highest-value repair targets
  1. `canon-utils/canon-route/src/executor.rs::emit_decision`
     - assign/persist `last_decision_trace_id` before `emit_route_selected_from_decision`
     - verify no later clear/reset races exist before route emission
  2. `canon-utils/canon-route/src/executor.rs::emit_route_selected_from_decision`
     - preserve strict invariant, but only after trace creation is guaranteed
     - add defensive diagnostics if trace production fails
  3. `canon-utils/canon-route/src/decision.rs`
     - replace `decide_from_json(...)` with explicit `SemanticStateSummary` input contract
  4. `canon-utils/canon-route/src/executor.rs`
     - rewire `emit_decision` to call the new semantic decision interface directly
  5. `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs` and `canon-utils/canon-route/src/helpers.rs`
     - remove or demote residual JSON/LLM route-authority helpers
  6. `canon-utils/canon-runtime/src/lib.rs` and `canon-utils/canon-runtime/src/bin/event_runtime.rs`
     - audit parent-id and fatal-halt behavior once route panic is removed

- blockers or missing evidence
  - current runtime verification is blocked by the `decision_trace` panic in `RouteExecutor`
  - recent event-log scans show decisions are still too sparse to confirm a healthy semantic loop
  - queue-derived routing influence remains secondary/unverified until the immediate panic and explicit semantic input wiring are repaired


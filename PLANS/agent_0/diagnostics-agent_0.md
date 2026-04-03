# Diagnostics Report - agent_0

## Inputs
- Spec: `PLANS/SPEC.md`
- Invariants: `PLANS/INVARIANTS.md`
- Violations: `VIOLATIONS.md`
- Event log: `state/event_log/event.tlog.d`

## Verifier reconciliation
Latest verifier summary says:
- fail-fast is enforced across pipeline stages
- RouteTick emission was introduced
- the decision→`RouteSelected` emission path exists

Unverified:
- RouteTick drives unconditional per-cycle decision execution
- routing is derived exclusively from `SemanticStateSummary`
- decision is executed every loop cycle

False according to verifier:
- decision stage is loop-driven
- spec-compliant semantic-state-driven control flow

This diagnostics pass confirms the verifier summary and sharpens it: the system now emits ticks, but RouteTick is not the authoritative per-cycle semantic decision driver. The route executor is still fundamentally event-driven, and route truth remains model_json-driven rather than `SemanticStateSummary`-driven.

## Event-log evidence
Latest canonical event-log scan:
- `.log`: `2079`
- `.idx`: `2079`
- `.time`: `2079`
- numeric segment gaps: `1456`
- median segment size: `6950`
- max segment size: `2255656`

Recent printable signals:
- `parent_ids=1933`
- `tick=1218`
- `invariant violation=254`
- `capabilityfailed=48`
- `capabilitycompleted=24`
- `decision=3`

Interpretation:
- tick activity is extremely high
- decision activity is extremely low relative to tick volume
- therefore RouteTick is not functioning as an unconditional per-cycle decision driver in the live system
- capability terminal events remain a much more plausible decision trigger surface than RouteTick

## Confirmed root causes

### 1. CRITICAL - RouteTick does not drive unconditional per-cycle decision execution
This is now directly supported by both event-log and source evidence.

#### Event-log evidence
- `tick=1218`
- `decision=3`

If RouteTick were truly driving unconditional decision execution each cycle, decision activity would track tick activity much more closely. It does not.

#### Exact source evidence
From `canon-utils/canon-runtime/src/bin/event_runtime.rs`:
- runtime explicitly calls `emit_tick()`

From `canon-utils/canon-route/src/executor.rs::filter`:
- `387: fn filter(&self) -> EventFilter {`
- `388:     EventFilter::All`

From `canon-utils/canon-route/src/executor.rs::on_event`:
- `441-443:`
  - `// CRITICAL: ensure routing pipeline executes for every event`
  - `// This triggers decision() -> RouteSelected emission`
  - `self.try_dispatch_route(event);`
- `451: if self.dispatch_in_progress && !matches!(event, RuntimeEvent::RouteTick(_)) {`
- `456: if matches!(event, RuntimeEvent::RouteTick(_)) && self.reroute_requested && !self.dispatch_in_progress {`
- `458:     self.try_dispatch_route(event);`
- `459:     return EventOutcome::NoOp("route_executor_reroute_tick");`

This proves:
- RouteExecutor is subscribed to all events, not specifically to RouteTick
- `try_dispatch_route(event)` is invoked generically from `on_event`
- RouteTick is used specially only to drain deferred reroutes after an emit stack completes
- RouteTick is not the primary or exclusive driver of decision execution

#### Diagnosis
The actual architecture is:
- decision/routing is event-driven through `on_event`
- RouteTick is only a helper for deferred reroute re-entry
- therefore the decision stage is not loop-driven in the canonical sense required by spec

### 2. CRITICAL - Routing authority is still model_json-driven rather than `SemanticStateSummary`-driven
This remains directly confirmed in source.

From `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs::evaluate_model_output`:
- `28: pub fn evaluate_model_output(&mut self, model_json: &str, signals: &RuntimeSignals) -> Result<(RouteSelection, GateResult), String> {`
- `31: let selection = parse_route_selection(model_json, &[RouteKind::Observe, RouteKind::Plan, RouteKind::Act, RouteKind::Verify, RouteKind::Conclude]).map_err(|err| err.to_string())?;`
- `33: let gate = self.gate.review(&selection, signals);`
- `36: Ok((selection, gate))`

This proves:
- route selection is parsed from `model_json`
- gating is applied to that parsed selection using `RuntimeSignals`
- `SemanticStateSummary` is not part of the authoritative decision input

Contract evidence from `VIOLATIONS.md`:
- `emit_decision` is triggered using `model_json` derived from `CapabilityResult`
- `decide_from_json` consumes `model_json` as primary input
- no direct use of `SemanticStateSummary` exists in the decision input path

Diagnosis:
- even if RouteTick were wired perfectly, the current decision authority would still be non-canonical because route truth comes from model output instead of semantic state

### 3. CRITICAL - RouteExecutor remains event-driven through `on_event`, not semantic-state-driven each cycle
Exact source evidence from `canon-utils/canon-route/src/executor.rs::on_event`:
- `404: fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {`
- `426: self.advance_control_state(event);`
- `427: self.ctx.update_from_event(event, &self.workspace);`
- `443: self.try_dispatch_route(event);`

This proves:
- RouteExecutor reacts to incoming events
- it mutates internal control/context from those events
- then it attempts routing from that event-driven state

Diagnosis:
- the control loop is still fundamentally `incoming event -> mutate context -> maybe route`
- this is not the same as `each cycle compute decision directly from SemanticStateSummary and then transition`

### 4. HIGH - RouteTick has a proven effect only as deferred reroute drain, not as canonical decision trigger
Exact source evidence:
- `451: if self.dispatch_in_progress && !matches!(event, RuntimeEvent::RouteTick(_)) {`
- `456: if matches!(event, RuntimeEvent::RouteTick(_)) && self.reroute_requested && !self.dispatch_in_progress {`
- `458:     self.try_dispatch_route(event);`
- `459:     return EventOutcome::NoOp("route_executor_reroute_tick");`

Diagnosis:
- the verified purpose of RouteTick in current code is to retry routing after a previous dispatch was deferred
- this is a reroute recovery mechanism, not a semantic per-cycle decision stage

### 5. HIGH - `try_dispatch_route` still derives from local/context/controller state rather than explicit semantic-state truth
Focused source body from `canon-utils/canon-route/src/executor.rs::try_dispatch_route` shows:
- `83: // semantic-only: remove planned_pending dependency`
- `84: let goal_unfinished = self.ctx.context_ready && self.ctx.mission_goal_spec.is_some() && !self.ctx.finish_ready;`
- `90-92:` plan presence is derived from local `RouteContext`

Diagnosis:
- route computation still depends on executor-local/context/controller state surfaces
- this is not equivalent to computing a fresh decision from `SemanticStateSummary`

### 6. HIGH - Invariant and successor-discharge failures remain live downstream symptoms
Event-log evidence:
- `invariant violation=254`

Diagnosis:
- these remain downstream symptoms of incorrect control authority and mixed trigger semantics
- until decision is rebuilt as per-cycle semantic-state-driven control, these failures are likely to persist even with more local guards

## True root problem
The actual root problem is:

> Canon still does not have a true per-cycle semantic decision stage. Instead, RouteExecutor reacts to arbitrary incoming events, updates local context, and attempts routing from that event-driven state; RouteTick only helps drain deferred reroutes, while route authority still comes from model_json via RouteController.

More precisely:
- RouteTick is emitted upstream
- but RouteTick is not the canonical unconditional decision trigger
- `on_event` is still the central event-driven gateway
- `try_dispatch_route(event)` is called from that event gateway
- authoritative route selection is still parsed from `model_json`
- `SemanticStateSummary` remains a contract requirement, not the implemented source of route truth

## Highest-priority repair order
1. Make RouteTick the explicit unconditional decision driver.
   - Add a clear `RuntimeEvent::RouteTick(_)` branch in `RouteExecutor::on_event` that runs decision every cycle.
   - Do not rely on RouteTick only as a reroute drain.

2. Separate event ingestion from decision execution.
   - Incoming events should update semantic state.
   - Decision should then run from semantic state on the cycle boundary, not opportunistically from arbitrary events.

3. Remove model-json-driven route authority.
   - Eliminate `RouteController::evaluate_model_output(model_json, signals)` from the authoritative decision path.
   - Remove `parse_route_selection(model_json, ...)` as the source of route truth.

4. Introduce a canonical semantic decision entrypoint.
   - Replace JSON-oriented route authority with something like `decide_from_semantic_state(summary: SemanticStateSummary)`.
   - Ensure `RouteDecision` is constructed exclusively from semantic truth.

5. Rebuild `try_dispatch_route` around semantic state.
   - It should consume canonical semantic state/invariants/policy outputs.
   - It should not derive route truth from local mirrors like `ctx.context_ready`, `mission_goal_spec`, `finish_ready`, controller signals, or capability-output JSON.

6. Re-verify downstream invariants after route authority is fixed.
   - successor discharge
   - observe/plan/act legality
   - duplicate/fanout handling
   - per-cycle decision execution

## Bottom line
Canon is not spec-compliant.

Strongest confirmed diagnosis:
- RouteTick exists, but current code shows it is only a deferred reroute helper. The actual decision path is still event-driven through `RouteExecutor::on_event`, not loop-driven each cycle.

Strongest secondary diagnosis:
- even if RouteTick were promoted to the true cycle driver, route truth would still be non-canonical because `RouteController::evaluate_model_output(model_json, signals)` parses route selection from model output instead of `SemanticStateSummary`.

# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 7)

1. **MAKE `RouteTick` THE EXPLICIT UNCONDITIONAL DECISION DRIVER**
   - Read `canon-utils/canon-route/src/executor.rs::on_event`.
   - Add a clear `RuntimeEvent::RouteTick(_)` branch that runs authoritative decision execution every cycle.
   - Do not keep `RouteTick` only as deferred reroute drain.
   - Test: run `cargo test -p canon-route`.

2. **SEPARATE EVENT INGESTION FROM DECISION EXECUTION**
   - Patch `canon-utils/canon-route/src/executor.rs` so arbitrary incoming events update semantic state or context but do not authoritatively trigger routing.
   - Remove or constrain `CapabilityCompleted` and `CapabilityFailed` as decision triggers.
   - Ensure a cycle can produce a decision without waiting for external capability completion or failure.
   - Test: run `cargo test -p canon-route`.

3. **REMOVE MODEL-JSON ROUTING AUTHORITY FROM THE SUPERVISOR**
   - Read `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs` around `RouteController::evaluate_model_output`.
   - Remove `parse_route_selection(model_json, ...)` from the authoritative decision path.
   - Ensure controller or signal logic is not the source of route truth.
   - Test: rebuild the touched crates after the interface change.

4. **REPLACE `decide_from_json` WITH A SEMANTIC DECISION ENTRYPOINT**
   - Read the crate that defines `decide_from_json` and `RouteDecision`.
   - Replace JSON or model-output decision entrypoints with `decide_from_semantic_state(summary: SemanticStateSummary)` or equivalent.
   - Update route-layer call sites to use the semantic entrypoint.
   - Test: run the affected crate tests plus `cargo test -p canon-route`.

5. **REBUILD `try_dispatch_route` AROUND CANONICAL SEMANTIC OUTPUTS**
   - Read `canon-utils/canon-route/src/executor.rs::try_dispatch_route`.
   - Remove local-context route derivation from `goal_unfinished`, `has_plan`, `ctx.signals`, controller state, and other local mirrors unless formally proven semantic projections.
   - Make `try_dispatch_route` consume canonical semantic decision outputs only, or remove it from authoritative routing entirely.
   - Eliminate panic-driven placeholder behavior.
   - Test: run `cargo test -p canon-route`.

6. **RE-VERIFY DOWNSTREAM CONTROL LAW ONLY AFTER LOOP-DRIVEN SEMANTIC AUTHORITY IS FIXED**
   - Then audit `canon-utils/canon-runtime/src/lib.rs`, `canon-utils/canon-runtime/src/bus.rs`, `canon-utils/canon-route/src/helpers.rs`, and `canon-utils/canon-loop/src/executor.rs`.
   - Focus on successor discharge, duplicate-control handling, invariant authority, and emission legality as downstream follow-on work.
   - Test: run the relevant crate tests after each scoped patch.

7. **RE-SCAN SOURCE AND EVENT LOG AFTER EACH PATCH SET**
   - Use `rg` to confirm removal of event-gated decision triggers, `decide_from_json`, `evaluate_model_output(model_json`, and `parse_route_selection(model_json` from the authoritative path.
   - Use Python from `/workspace/ai_sandbox/canon` to inspect fresh `state/event_log/event.tlog.d` segments for `tick`, `decision`, `RouteSelected`, `invariant violation`, `missing required successor`, and duplicate-control signals.
   - Confirm that decision activity tracks cycle ticks much more closely before declaring the lane complete.

## BLOCKED / NOT READY
- Do not prioritize blocked-emission success cleanup ahead of making `RouteTick` the canonical decision driver and removing model-json route authority.
- Do not prioritize downstream bus or hook-chain cleanup ahead of rebuilding the per-cycle semantic decision stage.
- Do not spend cycles on fallback planning or mini-agent prompt or response cleanup until the decision stage is rebuilt around `SemanticStateSummary`.

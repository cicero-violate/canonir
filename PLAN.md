# PLAN: Make RouteTick the Canonical Per-Cycle Decision Driver

## A. Authoritative Context

### Canonical Law
- `SemanticStateSummary` is the exclusive authority for route selection truth.
- `RouteTick` must be the explicit per-cycle decision boundary.
- Arbitrary incoming events may update state, but may not authoritatively trigger routing.
- `model_json`, capability output text, controller signals, and local executor mirrors are not routing truth.

### Current Verified State
- Fail-fast is enforced across pipeline stages.
- `RouteTick` emission was introduced upstream.
- The decision -> `RouteSelected` emission path exists structurally.

### Current Broken State
- Decision is not loop-driven.
- `RouteTick` is not the unconditional per-cycle decision driver.
- `RouteExecutor::on_event` is still the central event-driven gateway.
- `RouteController::evaluate_model_output(model_json, signals)` still parses route truth from `model_json`.
- `decide_from_json` remains in the route-layer interface surface.
- `try_dispatch_route` still derives from local or controller state instead of explicit semantic-state truth.

## B. Ranked Root Failures

### 0. `RouteTick` DOES NOT DRIVE UNCONDITIONAL PER-CYCLE DECISION EXECUTION (PRIMARY BLOCKER)
Evidence:
- Diagnostics show `tick=1218` and `decision=3`.
- Diagnostics show `RouteTick` is currently used only to drain deferred reroutes.

Required outcome:
- `RuntimeEvent::RouteTick(_)` must execute authoritative decision every cycle.

### 1. DECISION EXECUTION IS STILL EVENT-GATED THROUGH `on_event`
Evidence:
- Diagnostics show the current control architecture is `incoming event -> mutate context -> maybe route`.
- `CapabilityCompleted` and `CapabilityFailed` remain plausible decision-trigger surfaces.

Required outcome:
- Incoming events should update semantic state only.
- Decision must run from the cycle boundary, not opportunistically from arbitrary events.

### 2. ROUTE AUTHORITY IS STILL MODEL-JSON-DRIVEN, NOT `SemanticStateSummary`-DRIVEN
Evidence:
- `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs::evaluate_model_output(model_json, signals)` calls `parse_route_selection(model_json, ...)`.
- Diagnostics confirm `SemanticStateSummary` is absent from the authoritative decision input.

Required outcome:
- Route truth must be computed directly from `SemanticStateSummary`, invariant context, and canonical policy evaluation.

### 3. `decide_from_json` REMAINS A NON-CANONICAL DECISION INTERFACE
Evidence:
- `canon-utils/canon-route/src/executor.rs` still imports `decide_from_json`.

Required outcome:
- Replace JSON or model-driven decision interfaces with a semantic entrypoint such as `decide_from_semantic_state(summary: SemanticStateSummary)`.

### 4. `try_dispatch_route` IS STILL CENTERED ON LOCAL OR CONTROLLER STATE
Evidence:
- Diagnostics cite `goal_unfinished`, `has_plan`, and local `RouteContext` surfaces inside `try_dispatch_route`.

Required outcome:
- `try_dispatch_route` must consume canonical semantic decision outputs only, or be removed from the route-authority path entirely.

### 5. DOWNSTREAM INVARIANT, SUCCESSOR, AND EMISSION FAILURES ARE LIVE BUT SECONDARY
Evidence:
- Diagnostics still show live invariant violations.
- Diagnostics identify these as downstream symptoms of incorrect decision authority and trigger semantics.

Required outcome:
- Re-verify successor discharge, duplicate-control handling, observe/plan/act legality, and emission behavior only after loop-driven semantic authority is fixed.

## C. Dependency Order
1. Make `RouteTick` the explicit unconditional per-cycle decision driver in `canon-utils/canon-route/src/executor.rs`.
2. Separate event ingestion from decision execution so arbitrary incoming events update semantic state but do not authoritatively trigger routing.
3. Remove model-json-driven route authority from `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs`.
4. Replace `decide_from_json` with a semantic-state-driven decision entrypoint and route decision data flow.
5. Rebuild `try_dispatch_route` so it consumes semantic decision outputs instead of local or controller mirrors.
6. Only after steps 1 through 5, re-verify downstream invariants, successor discharge, duplicate-control handling, and emission legality.

## D. READY NOW

### Executor: executor_pool
1. Read and patch `canon-utils/canon-route/src/executor.rs::on_event` so `RuntimeEvent::RouteTick(_)` is the explicit unconditional driver of decision execution every cycle.
   - Add a clear `RouteTick` branch that runs authoritative decision each cycle.
   - Do not keep `RouteTick` only as deferred reroute drain.
   - Test: run `cargo test -p canon-route`.

2. Read and patch `canon-utils/canon-route/src/executor.rs` so arbitrary incoming events no longer authoritatively trigger routing.
   - Keep event ingestion for semantic-state or context updates.
   - Move authoritative decision execution to the cycle boundary.
   - Remove or constrain `CapabilityCompleted` and `CapabilityFailed` as decision triggers.
   - Test: run `cargo test -p canon-route`.

3. Read and patch `canon-utils/canon-runtime-supervisor/src/judgment_loop.rs` to remove `RouteController::evaluate_model_output(model_json, signals)` from route authority.
   - Delete or isolate `parse_route_selection(model_json, ...)` from the canonical decision path.
   - Ensure controller or signal logic is not the source of route truth.
   - Test: rebuild the touched crates after the interface change.

4. Read and patch the decision interface so `decide_from_json` no longer exists on the authoritative path.
   - Start with `canon-utils/canon-route/src/executor.rs` and the crate that defines the decision API.
   - Replace JSON or model-driven decision entrypoints with `decide_from_semantic_state(summary: SemanticStateSummary)` or equivalent.
   - Remove any residual model-json dependency from `RouteDecision` construction.
   - Test: run the touched crate tests plus `cargo test -p canon-route`.

5. Read and patch `canon-utils/canon-route/src/executor.rs::try_dispatch_route` so it no longer derives route truth from `RouteContext`, `RouteController`, `ctx.signals`, `goal_unfinished`, `has_plan`, or similar local mirrors.
   - Make it consume canonical semantic decision outputs only, or remove it from authoritative routing entirely.
   - Eliminate panic-driven placeholder behavior.
   - Test: run `cargo test -p canon-route`.

6. After steps 1 through 5, audit downstream control correctness in this order:
   - `canon-utils/canon-runtime/src/lib.rs`
   - `canon-utils/canon-runtime/src/bus.rs`
   - `canon-utils/canon-route/src/helpers.rs`
   - `canon-utils/canon-loop/src/executor.rs`
   Focus only on successor discharge, duplicate-control handling, invariant authority, and emission legality as follow-on work.

7. After each patch set, run targeted verification and source scans.
   - Use `rg` to confirm the authoritative path no longer depends on `decide_from_json`, `evaluate_model_output(model_json`, `parse_route_selection(model_json`, or event-gated decision triggers.
   - Use Python to inspect fresh `state/event_log/event.tlog.d` segments for `tick`, `decision`, `RouteSelected`, `invariant violation`, `missing required successor`, and duplicate-control signals.
   - Confirm decision activity tracks cycle ticks much more closely before declaring the lane complete.

## E. BLOCKED / NOT READY YET
- Do not prioritize blocked-emission success cleanup ahead of fixing loop-driven semantic decision authority.
- Do not prioritize downstream bus or hook-chain cleanup ahead of making `RouteTick` the canonical decision driver and removing model-json route authority.
- Do not spend cycles on fallback planning or mini-agent prompt/response cleanup until the decision stage is rebuilt around `SemanticStateSummary` and true per-cycle execution.

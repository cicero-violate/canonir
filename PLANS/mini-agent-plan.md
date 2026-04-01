# PLAN: Canon Control-Flow Stabilization

## Objective
Stabilize canonical control flow in `/workspace/ai_sandbox/canon/canon-utils` so that:
- `RouteSelected -> required successor` is always correct.
- `PlanningCompleted(planned_count=0, status=missing_semantic_context) -> RouteSelected(observe) -> LoopObserved`.
- `RouteSelected(act)` only occurs when real planned work exists.
- No fake scheduler seeding, forced `Act`, manual `RequestDispatch`, or suppressed `loop_acted` hacks remain.
- Duplicate observe / duplicate forwarding noise is eliminated.

## Constraints
- Maintain build correctness.
- Preserve event-log successor invariants.
- Do not introduce new synthetic control events.
- Do not rely on stale `scheduler_len`; prefer `planned_pending` / `planned_count`.
- Normalize route matching at read boundaries (`"Act"` vs `"act"`).
- Prefer narrow control-flow repairs over broad rewrites.

## Current State
Completed and should remain true:
- [x] `planning_completed` is appended to tlog again ✓ done
- [x] `PlanningCompleted(planned_count=0, status=missing_semantic_context)` recovers via `RouteSelected(observe)` ✓ done
- [x] `LoopObserved` is emitted and discharges the expected successor after observe recovery ✓ done
- [x] Route-string casing mismatch for stage dispatch / loop executor was repaired ✓ done

Still suspected / incomplete:
- [ ] Remove remaining non-canonical routing hacks from `canon-route/src/executor.rs`
- [ ] Remove duplicate observe / duplicate forwarding / duplicate fanout noise
- [ ] Ensure `planned_pending := planned_count` is the authoritative planning-work signal everywhere
- [ ] Ensure no stale local scheduler mirror can force `Act` or `Plan`
- [ ] Verify `RouteSelected(act) -> LoopActed` only when executable work exists
- [ ] Verify goal-generation / mini-agent prompt shells do not wrap raw-markdown tasks in JSON-only contracts

## Primary Files
- `canon-route/src/executor.rs`
- `canon-route/src/context.rs`
- `canon-route/src/policy.rs`
- `canon-loop/src/stage/mod.rs`
- `canon-loop/src/executor.rs`
- `canon-loop/src/stage/plan.rs`
- `canon-loop/src/stage/act.rs`
- `canon-runtime/src/lib.rs`
- `canon-runtime/src/consumers/dispatch_consumer.rs`
- `canon-runtime/src/consumers/capability_executor.rs`
- `canon-mini-agent/src/main.rs`

## Workstream 1 — Route Executor Cleanup
- [ ] Audit `canon-route/src/executor.rs` for any remaining:
  - forced `PlanningCompleted -> Act`
  - manual `RequestDispatch` emission
  - fake `scheduler_len` mutations / direct seeding
  - successor suppression (`loop_acted`, `route_selected`, etc.)
  - decision overrides that bypass normal policy
- [ ] Remove each remaining hack and keep routing driven by:
  - `canon_invariant::decide(...)`
  - `evaluate_route_event_dispatch(...)`
  - actual `planned_pending` / `planned_count`
- [ ] Re-read the exact post-patch code and confirm no synthetic route / dispatch shortcuts remain.

## Workstream 2 — Planning/Queue Truth
- [ ] Audit `canon-route/src/context.rs` and `canon-route/src/policy.rs`.
- [ ] Ensure `record_planning_completion(status, planned_count)` updates:
  - `planned_pending`
  - `scheduler_len` only as a mirrored consequence, never as the source of truth
- [ ] Replace any route decision logic that still treats `scheduler_len` as authoritative when `planned_pending` / `planned_count` is available.
- [ ] Verify that zero-task planning statuses do not re-enter `Act`.

## Workstream 3 — Observe Recovery and Duplicate Fanout
- [ ] Audit duplicate-delivery surfaces in:
  - `canon-runtime/src/consumers/dispatch_consumer.rs`
  - `canon-runtime/src/lib.rs`
  - `canon-runtime/src/bus.rs`
  - `canon-loop/src/executor.rs`
- [ ] Identify why `loop_observed` is delivered multiple times per recovery cycle.
- [ ] Remove duplicate forwarding / fanout paths while preserving required delivery.
- [ ] Verify that one `RouteSelected(observe)` yields one canonical `LoopObserved` control successor.

## Workstream 4 — Act Path Correctness
- [ ] Audit `canon-loop/src/stage/act.rs` and `canon-loop/src/executor.rs`.
- [ ] Confirm `act::execute_dispatch` only enters when real work exists.
- [ ] Confirm `dispatch_plan(...)` emits executable events only for actionable plans.
- [ ] Confirm `LoopActed` is emitted only when a real `ToolResult` exists.
- [ ] Confirm no synthetic `Act` route is emitted for zero-task planning outcomes.

## Workstream 5 — Prompt / Agent-Shell Contract Audit
- [ ] Audit `canon-mini-agent/src/main.rs` and any goal-generation path.
- [ ] Verify no raw-markdown generation task is wrapped in a JSON-only action shell.
- [ ] If a role requires raw markdown, ensure it does not pass through `parse_actions(...)`.
- [ ] Document the contract boundary clearly in code comments or plan notes.

## Verification Commands
Use these after each meaningful fix:
1. `cargo build -p canon-route -p canon-loop -p canon-runtime -p canon-mini-agent`
2. `cargo run --bin canon-runtime-supervisor 2>&1 | tee /tmp/runtime.trace`
3. `rg -n "planning_completed|route_selected|loop_observed|loop_acted|observe_suppressed_due_to_pending_successor|act_bootstrap|EXEC CORE|capability_executor" /tmp/runtime.trace`
4. `rg -n "RequestDispatch|seeding scheduler directly|forcing RouteSelected|auto-route from planning|skip.*loop_acted|suppress.*loop_acted" canon-utils/canon-route/src canon-utils/canon-runtime/src canon-utils/canon-loop/src`

## Success Criteria
- [ ] `PlanningCompleted(0, missing_semantic_context) -> RouteSelected(observe) -> LoopObserved` occurs cleanly
- [ ] `loop_observed` is not duplicated by fanout / forwarding hacks
- [ ] `RouteSelected(act)` only occurs when `planned_count > 0` / real work exists
- [ ] `RouteSelected(act) -> LoopActed` is restored as a real required successor when act work exists
- [ ] No fake scheduler seeding remains
- [ ] No manual `RequestDispatch` or synthetic forced `Act` remains in route control
- [ ] Build passes for the touched crates
- [ ] Trace shows canonical control succession without deadlocks or duplicate control spam

## Executor Notes
- Always read a file before patching it.
- Patch one surface at a time.
- After each completed item, immediately mark it done in this file:
  - `- [ ] item` -> `- [x] item ✓ done`
- Prefer exact-context patches with 3+ unchanged lines.
- When a patch fails, re-read the exact line range before retrying.

## Verifier Notes
The verifier should reject any claimed completion that still leaves:
- forced route emissions
- fake queue mutations
- suppressed successors
- duplicate observe delivery
- JSON shell contamination of raw-markdown tasks

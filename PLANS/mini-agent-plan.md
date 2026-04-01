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
- [x] `LoopObserved` is emitted and discharges the expected successor after observe recovery ✓ done
- [x] Route-string casing mismatch for stage dispatch / loop executor was repaired ✓ done

Still suspected / incomplete:
- [x] Remove remaining non-canonical routing hacks from `canon-route/src/executor.rs` ✓ audit complete
  1. Open file: canon-route/src/executor.rs
  2. Run: rg -n "RequestDispatch|force|seed|scheduler_len|override|suppress" canon-route/src/executor.rs
  3. Enumerate all locations performing manual routing, dispatch emission, or scheduler mutation
  4. For each location, trace whether it bypasses canon_invariant::decide(...) or evaluate_route_event_dispatch(...)
  5. Mark each occurrence as VALID (policy-driven) or INVALID (synthetic/hack)
- [ ] Remove duplicate observe / duplicate forwarding / duplicate fanout noise
 - [x] Remove duplicate observe / duplicate forwarding / duplicate fanout noise  ✓ partial (dispatch + bus dedup applied)
  1. Open files:
     - canon-runtime/src/consumers/dispatch_consumer.rs
     - canon-runtime/src/lib.rs
     - canon-runtime/src/bus.rs
     - canon-loop/src/executor.rs
  
## DAG — Execution Ordering (Derived from Diagnostics)

### Tier 0 (BLOCKING INVARIANTS)
- [x] Enforce scheduler_len == 0 ⇒ Observe (block Act)  ✓ done
  1. Add invariant in canon-invariant/src/lib.rs: scheduler_len == 0 → Decision::Observe
  2. Add guard in canon-route/src/executor.rs to block Act when scheduler_len == 0
  3. Ensure no override paths bypass this guard

- [x] Make LoopObserved emission unconditional  ✓ done
  1. Open canon-loop/src/stage/observe.rs
  2. Ensure ALL return paths emit LoopObserved
  3. Remove conditional skips (Noop / placeholder cases)

### Tier 1 (OBSERVABILITY)
- [x] Implement DECIDE + ROUTE trace coverage  ✓ done
  1. Add DECIDE trace in canon-invariant/src/lib.rs
  2. Add ROUTE trace in canon-route/src/executor.rs
  3. Ensure every route_selected has both traces

### Tier 1 (OBSERVABILITY)
- [ ] Implement DECIDE + ROUTE trace coverage
  1. Add DECIDE trace in canon-invariant/src/lib.rs
  2. Add ROUTE trace in canon-route/src/executor.rs
  3. Ensure every route_selected has both traces

### READY NOW (EXECUTOR WINDOW)
1. [x] Fix DECIDE + ROUTE trace regression (CRITICAL, 28 missing)  ✓ done
   1. Open canon-invariant/src/lib.rs and locate decision emission
   2. Ensure EVERY decision path emits DECIDE trace with trace_id + payload
   3. Open canon-route/src/executor.rs and locate ALL RouteSelected emit sites
   4. Ensure EACH emit is preceded by ROUTE trace (no early-return bypass)
   5. Run: rg -n "ROUTE TRACE|DECIDE TRACE" canon-utils and confirm coverage

2. [x] Make LoopObserved emission strictly unconditional (24 missing)  ✓ done
   1. Open canon-loop/src/stage/observe.rs
   2. Enumerate ALL return paths (rg -n "return" observe.rs)
   3. Ensure LoopObserved is emitted immediately before EVERY return
   4. Remove Noop / conditional suppression branches
   5. Add debug log: [OBSERVE TRACE] emitted LoopObserved

3. Enforce event lifecycle completion (successor discharge gaps)
   1. Search: rg -n "discharge|complete|finalize" canon-runtime canon-loop
   2. Trace lifecycle: emit → route → act → discharge
   3. Identify missing discharge paths
   4. Add explicit discharge step where missing
   5. Verify each emitted event reaches terminal state

4. Validate scheduler invariant enforcement (regression check)
   1. Search: rg -n "scheduler_len" canon-utils
   2. Confirm all decision paths use planned_pending or guarded scheduler_len
   3. Ensure no path routes Act when scheduler_len == 0
   4. Add log: [DECIDE CHECK] scheduler_len={} decision={}
   5. Verify no bypass paths exist
- [ ] Ensure `planned_pending := planned_count` is the authoritative planning-work signal everywhere
  1. Open files:
     - canon-route/src/context.rs
     - canon-route/src/policy.rs
  2. Run: rg -n "planned_pending|planned_count|scheduler_len" canon-route/src
  3. Identify all assignments and reads of planned_pending, planned_count, and scheduler_len
  4. Trace whether any decision logic uses scheduler_len directly instead of planned_pending/planned_count
  5. Mark all locations where scheduler_len is treated as authoritative rather than derived
- [ ] Ensure no stale local scheduler mirror can force `Act` or `Plan`
  1. Open files:
     - canon-route/src/context.rs
     - canon-loop/src/context.rs
  2. Run: rg -n "scheduler_len" canon-utils
  3. Identify all locations where scheduler_len is stored, cached, or recomputed
  4. Trace whether any local copies diverge from planned_pending or planned_count
  5. Mark any paths where stale scheduler_len could incorrectly trigger Act or Plan
- [ ] Verify `RouteSelected(act) -> LoopActed` only when executable work exists
  1. Open files:
     - canon-loop/src/stage/act.rs
     - canon-loop/src/executor.rs
  2. Run: rg -n "LoopActed" canon-utils
  3. Identify all emission sites of LoopActed events
  4. For each site, trace backward to confirm presence of actual ToolResult or executed work
  5. Add guard before emission:
     if no executable work exists → do not emit LoopActed
  6. Verify no paths emit LoopActed after zero-task planning or empty scheduler
- [ ] Verify goal-generation / mini-agent prompt shells do not wrap raw-markdown tasks in JSON-only contracts
  1. Open file: canon-mini-agent/src/main.rs
  2. Search: rg -n "parse_actions|json|markdown" canon-mini-agent
  3. Identify all paths where generated goals/tasks are wrapped into JSON structures
  4. Trace whether raw-markdown tasks are incorrectly passed through JSON-only parsing (parse_actions)
  5. Ensure raw markdown outputs bypass JSON parsing and are emitted directly
  6. Add comment clarifying boundary: JSON actions vs raw markdown tasks

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
  1. Open file: canon-route/src/executor.rs
  2. Run: rg -n "PlanningCompleted|RequestDispatch|scheduler_len|force|override|suppress" canon-route/src/executor.rs
  3. For each match, trace surrounding logic (±20 lines) to identify non-canonical routing behavior
  4. Classify each occurrence into one of:
     - forced route
     - synthetic dispatch
     - scheduler mutation
     - suppression logic
  5. Record exact line numbers and behavior for each violation
- [ ] Remove each remaining hack and keep routing driven by:
  - `canon_invariant::decide(...)`
  - `evaluate_route_event_dispatch(...)`
  - actual `planned_pending` / `planned_count`
  1. For each identified violation, rewrite logic to defer to canon_invariant::decide(...)
  2. Remove any direct mutation of scheduler_len unless derived from planned_pending
  3. Replace manual dispatch emission with evaluate_route_event_dispatch(...)
  4. Ensure no branch directly emits RouteSelected without going through policy
  5. Re-run rg to confirm removed patterns no longer exist
- [ ] Re-read the exact post-patch code and confirm no synthetic route / dispatch shortcuts remain.
  1. Re-open canon-route/src/executor.rs
  2. Scan full file top-to-bottom for any remaining manual routing logic
  3. Verify all route decisions originate from canon_invariant::decide or policy layer
  4. Run: rg -n "RequestDispatch|force|override|suppress" canon-route/src/executor.rs
  5. Confirm zero matches for synthetic routing patterns

## Workstream 2 — Planning/Queue Truth
- [ ] Audit `canon-route/src/context.rs` and `canon-route/src/policy.rs`.
  1. Open files:
     - canon-route/src/context.rs
     - canon-route/src/policy.rs
  2. Run: rg -n "planned_pending|planned_count|scheduler_len" canon-route/src
  3. Identify all locations where scheduler_len is read for decision-making
  4. Trace each usage to determine whether it should instead use planned_pending or planned_count
  5. Record mismatches where scheduler_len is treated as authoritative
- [ ] Ensure `record_planning_completion(status, planned_count)` updates:
  - `planned_pending`
  - `scheduler_len` only as a mirrored consequence, never as the source of truth
  1. Locate implementation of record_planning_completion in context.rs
  2. Verify planned_pending is set directly from planned_count
  3. Ensure scheduler_len is derived from planned_pending, not independently mutated
  4. Remove any code paths that update scheduler_len without updating planned_pending
  5. Add comment enforcing invariant: planned_pending is source of truth
- [ ] Replace any route decision logic that still treats `scheduler_len` as authoritative when `planned_pending` / `planned_count` is available.
  1. In policy.rs, locate decision logic (e.g., decide or equivalent)
  2. Replace conditions like scheduler_len > 0 with planned_pending > 0
  3. Ensure zero planned_pending always routes to Observe
  4. Normalize all decision branches to use planned_pending consistently
  5. Re-run rg to confirm scheduler_len is not used in decision conditions
- [ ] Verify that zero-task planning statuses do not re-enter `Act`.
  1. Search: rg -n "missing_semantic_context|planned_count=0" canon-route canon-loop
  2. Trace routing path after PlanningCompleted(planned_count=0)
  3. Ensure decision resolves to Observe, never Act
  4. Add guard in policy/executor: if planned_pending == 0 ⇒ force Observe
  5. Confirm no fallback path re-routes to Act

## Workstream 3 — Observe Recovery and Duplicate Fanout
- [ ] Audit duplicate-delivery surfaces in:
  - `canon-runtime/src/consumers/dispatch_consumer.rs`
  - `canon-runtime/src/lib.rs`
  - `canon-runtime/src/bus.rs`
  - `canon-loop/src/executor.rs`
  1. Open each file listed above
  2. Run: rg -n "loop_observed|emit|dispatch|fanout|forward" canon-runtime canon-loop
  3. Identify all emission and forwarding points for LoopObserved and related events
  4. Trace call chains to detect multiple emission paths for the same logical event
  5. Record exact duplication points and their triggering conditions
- [ ] Identify why `loop_observed` is delivered multiple times per recovery cycle.
  1. Correlate emission sites with runtime trace (search for repeated loop_observed entries)
  2. Map each duplicate to a specific code path (consumer, bus, executor, etc.)
  3. Determine whether duplication is due to:
     - multiple emitters
     - bus fanout
     - consumer re-processing
  4. Confirm whether duplication occurs before or after routing layer
  5. Document root cause per duplication source
- [ ] Remove duplicate forwarding / fanout paths while preserving required delivery.
  1. For each duplication source, choose a single canonical emission point
  2. Remove or guard redundant emit/forward calls
  3. Ensure bus or consumer does not re-broadcast already delivered events
  4. Add idempotency guard if necessary (e.g., trace_id or event_id check)
  5. Verify no loss of required downstream delivery
- [ ] Verify that one `RouteSelected(observe)` yields one canonical `LoopObserved` control successor.
  1. Run runtime: cargo run --bin canon-runtime-supervisor
  2. Capture trace and search: rg -n "route_selected.*observe|loop_observed" trace.log
  3. For each RouteSelected(observe), confirm exactly one LoopObserved follows
  4. Ensure no duplicate or missing LoopObserved entries
  5. Confirm successor discharge occurs exactly once

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

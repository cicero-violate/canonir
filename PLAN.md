# PLAN: Remove Queue-Local Control Authority and Rustc Capture Artifact Leaks

## A. Authoritative Context

### Canonical Law
- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- Queue counters such as `scheduler_len`, `planned_count`-seeding hacks, and executor-local fallback injection are not root truth.
- Zero-work, blocked-work, and repair-required states must be represented through canonical semantic/control events, not hidden scheduler seeding.

### Verified Progress This Cycle
- Latest verifier summary says `decision input restricted to SemanticStateSummary at type level`.
- Latest verifier summary says `semantic_summary assigned from LoopObserved event`.
- Latest verifier summary says `no additional mutation sites for semantic_summary found`.
- Core architectural concern is no longer semantic-state routing authority; remaining unverified work is runtime behavior and fresh source-side control drift.

### Still Broken
- `canon-utils/canon-loop/src/stage/plan.rs::execute_complete` still injects fallback work when `ctx.pending_plan` is `None` and `ctx.scheduler.len() == 0`.
- `canon-utils/canon-loop/src/stage/plan.rs` still asserts `scheduler_len_after > 0` after fallback insertion, preserving queue-count authority over control recovery.
- `canon-utils/canon-runtime/src/bin/harness_repair.rs::local_planner_fallback` still constructs fallback/debug strings in a shape that diagnostics tie to fresh rustc capture invariant violations.
- Legacy semantic decision naming/wiring drift remains in `canon-utils/canon-route/src/decision.rs`, `canon-utils/canon-route/src/executor.rs`, and `canon-utils/canon-route/src/lib.rs`, but this is now follow-on cleanup rather than the top blocker.

## B. Ranked Root Failures

### 1. PLAN STAGE STILL USES QUEUE-LOCAL SCHEDULER STATE AS CONTROL AUTHORITY (CRITICAL)
Evidence:
- `canon-utils/canon-loop/src/stage/plan.rs:173-209`:
  - `execute_complete(...)` checks `ctx.scheduler.len()` when `ctx.pending_plan.take()` is `None`.
  - It injects a fallback scheduled task and emits `PlanningCompleted { planned_count: 1, status: "complete_fallback" }`.
- `canon-utils/canon-loop/src/stage/plan.rs:643-650`:
  - `let scheduler_len_after = ctx.scheduler.len();`
  - `assert!(scheduler_len_after > 0, "Plan produced zero tasks — deadlock risk");`
  - Emits `PlanningCompleted { planned_count: 1, status: "fallback" }`.

Required outcome:
- Remove queue-count-driven fallback injection and queue-count assertions as control authority.
- Represent zero-task, blocked, or no-work outcomes via canonical semantic/control events and explicit statuses.
- Ensure plan completion truth is derived from semantic state and validated planner outcomes, not hidden scheduler seeding.

### 2. HARNESS FALLBACK STRING GENERATION IS LEAKING DEBUG/CLOSURE ARTIFACTS INTO RUSTC CAPTURE (HIGH)
Evidence:
- Diagnostics tie fresh invariant violations in `state/event_log/event.tlog.d/00000000000000012400.log` and `00000000000000012437.log` to leaked fallback/debug string shapes.
- `canon-utils/canon-runtime/src/bin/harness_repair.rs:355-384`:
  - `prompt_primary` is formatted for debug output.
  - `fallback` is synthesized via `extract_primary_file_line(prompt).map(|(path, line)| format!(...))` inside the fallback chain.
  - Diagnostics match this closure/formatted source shape to the leaked interner name artifact.

Required outcome:
- Precompute and sanitize fallback action strings before any debug/capture path.
- Eliminate closure-shaped or debug-shaped formatted expressions from values that can cross the rustc capture boundary.
- Audit the fallback/capture boundary so diagnostic prints cannot become interner names.

### 3. LEGACY DECISION API NAMING AND NON-SEMANTIC SCAFFOLDING STILL REMAIN (MEDIUM)
Evidence:
- Diagnostics report `canon-utils/canon-route/src/decision.rs` still uses `decide_from_json(...)` naming and still carries `_model_json` / `_controller` scaffolding.
- Prior live source reads showed executor/lib wiring still exports and calls the stale API shape.

Required outcome:
- Rename the decision API to a semantic-state authority name.
- Remove stale `RouteController` / `_model_json` scaffolding from decision wiring.
- Keep this behind the two active high-impact failures above.

## C. Dependency-Ordered Work

### Phase 1 — Remove scheduler-count control authority from the plan stage
1. Read `canon-utils/canon-loop/src/stage/plan.rs` around `execute_complete`, empty-plan handling, and `PlanningCompleted` emission.
2. Patch `execute_complete(...)` so `pending_plan = None` does not seed scheduler fallback work from `ctx.scheduler.len()`.
3. Replace queue-count-driven recovery with canonical semantic/control outcomes such as explicit zero-work / blocked / no-action statuses and lawful events.
4. Remove `scheduler_len_after` assertions as control truth.
5. Ensure `PlanningCompleted.planned_count` reflects actual emitted planned actions, not synthetic scheduler seeding.
6. Test: `cargo test -p canon-loop` and any directly affected workspace tests.

### Phase 2 — Stop rustc capture artifact leaks from harness fallback generation
1. Read `canon-utils/canon-runtime/src/bin/harness_repair.rs` around `local_planner_fallback`, `derive_post_read_action`, and nearby debug printing.
2. Patch fallback generation so extracted file/line data is precomputed into sanitized plain strings before formatting or logging.
3. Remove any closure-shaped or debug-shaped formatted expression from values that can enter capture/interner paths.
4. Audit nearby debug prints so logged fallback artifacts cannot be captured as names.
5. Test: `cargo test -p canon-runtime --bin harness_repair` if available, otherwise `cargo test -p canon-runtime`.

### Phase 3 — Clean up stale semantic decision API naming and scaffolding
1. Read `canon-utils/canon-route/src/decision.rs`, `canon-utils/canon-route/src/executor.rs`, and `canon-utils/canon-route/src/lib.rs`.
2. Rename `decide_from_json` to a semantic-state authority name such as `decide_from_semantic_state`.
3. Remove `_model_json`, `RouteController`, and other obsolete non-authoritative scaffolding from the decision interface and callers.
4. Test: `cargo test -p canon-route`.

## D. READY NOW

1. REMOVE QUEUE-COUNT FALLBACK AUTHORITY FROM `canon-loop` PLAN STAGE
   - Read `canon-utils/canon-loop/src/stage/plan.rs` around `execute_complete`, empty-plan fallback emission, and `PlanningCompleted` construction.
   - Patch `execute_complete(...)` so `ctx.pending_plan = None` does not inject fallback scheduled tasks based on `ctx.scheduler.len()`.
   - Remove `scheduler_len_after`-based assertions and any synthetic `planned_count: 1` fallback completion that is not backed by real planned actions.
   - Encode zero-work / blocked / no-action outcomes as canonical semantic/control results instead of queue seeding.
   - Test: `cargo test -p canon-loop`.

2. REMOVE RUSTC CAPTURE ARTIFACT LEAKS FROM `local_planner_fallback`
   - Read `canon-utils/canon-runtime/src/bin/harness_repair.rs` around `local_planner_fallback`, `derive_post_read_action`, and fallback/debug logging.
   - Patch fallback generation so extracted prompt file/line information is precomputed and sanitized before formatting.
   - Remove closure-shaped fallback expression construction from any value that can cross capture/logging boundaries.
   - Audit `eprintln!` payloads in the same path so debug artifacts cannot become interner names.
   - Test: `cargo test -p canon-runtime`.

3. RUN TARGETED STRUCTURAL VERIFICATION ON THE TWO ACTIVE FAILURES
   - Run `rg -n "scheduler\.len\(|complete_fallback|status: \"fallback\"|status: \"complete_fallback\"" canon-utils/canon-loop/src/stage/plan.rs`.
   - Run `rg -n "local_planner_fallback|extract_primary_file_line\(prompt\)|local_planner_debug|local_planner_dispatch" canon-utils/canon-runtime/src/bin/harness_repair.rs`.
   - Confirm queue-count control recovery and capture-leak patterns are removed or reduced to non-authoritative safe forms.
   - Run the relevant crate tests after each patch.

## E. Blocked Follow-On Work
- Do not prioritize stale semantic-boundary rewrites first; verifier evidence says core semantic-state authority is already restored.
- Keep decision API rename/scaffolding cleanup behind the plan-stage control fix and harness capture fix unless those tasks expose a direct dependency.

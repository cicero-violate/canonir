# EXECUTOR PLAN (executor_pool)

## READY NOW (MAX 10)

1. REMOVE QUEUE-COUNT FALLBACK AUTHORITY FROM `canon-loop` PLAN STAGE
   - Read `canon-utils/canon-loop/src/stage/plan.rs` around `execute_complete`, empty-plan fallback handling, and `PlanningCompleted` emission.
   - Patch `execute_complete(...)` so `ctx.pending_plan = None` does not inject fallback scheduled tasks from `ctx.scheduler.len()`.
   - Remove `scheduler_len_after` assertions and synthetic fallback `planned_count: 1` emissions that are not backed by real planned actions.
   - Replace queue-count recovery with canonical semantic/control outcomes for zero-work, blocked, or no-action cases.
   - Test: `cargo test -p canon-loop`.

2. REMOVE RUSTC CAPTURE ARTIFACT LEAKS FROM HARNESS FALLBACK GENERATION
   - Read `canon-utils/canon-runtime/src/bin/harness_repair.rs` around `local_planner_fallback`, `derive_post_read_action`, and debug logging.
   - Patch fallback generation so file/line extraction is precomputed and sanitized before formatting or logging.
   - Eliminate closure-shaped or debug-shaped fallback expression strings from any value that can enter capture/interner paths.
   - Audit adjacent `eprintln!` payloads and dispatch strings for capture safety.
   - Test: `cargo test -p canon-runtime`.

3. VERIFY THE TWO ACTIVE ROOT FIXES BEFORE FOLLOW-ON CLEANUP
   - Run `rg -n "scheduler\.len\(|complete_fallback|status: \"fallback\"|status: \"complete_fallback\"" canon-utils/canon-loop/src/stage/plan.rs`.
   - Run `rg -n "local_planner_fallback|extract_primary_file_line\(prompt\)|local_planner_debug|local_planner_dispatch" canon-utils/canon-runtime/src/bin/harness_repair.rs`.
   - Confirm queue-count authority and capture-leak patterns are gone or reduced to safe non-authoritative forms.
   - Re-run crate tests after each patch.

4. CLEAN UP STALE SEMANTIC DECISION API NAMING AFTER ROOT FIXES LAND
   - Read `canon-utils/canon-route/src/decision.rs`, `canon-utils/canon-route/src/executor.rs`, and `canon-utils/canon-route/src/lib.rs`.
   - Rename `decide_from_json` to a semantic-state authority name and remove obsolete `_model_json` / `RouteController` scaffolding.
   - Test: `cargo test -p canon-route`.

## BLOCKED UNTIL READY-NOW WORK LANDS
- Do not spend this lane on generic runtime guarantees, broad event-bus proofs, or cosmetic route naming cleanup before Tasks 1-3 complete.
- Treat semantic-boundary work as follow-on cleanup unless a current root fix exposes a direct dependency.

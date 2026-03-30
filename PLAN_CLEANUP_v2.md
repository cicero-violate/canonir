# PLAN: Continue Simplification After Cleanup Progress

## Status
Completed:
1. Behavior map and initial cleanup
2. Route policy truth-alignment and naming fixes
3. Loop retry-policy narrowing
4. Planner hint centralization
5. Route executor thinning
6. Loop executor helper extraction pass

## Next Objective
Finish reducing remaining large orchestration blocks and lock the boundaries.

## Phase 6 — Finish Loop Executor Simplification
- Extract remaining large `on_event()` branches in `canon-loop/src/executor.rs`
- Prioritize:
  - `ErrorOccurred`
  - `PlanningCompleted`
  - small state-update branches (`RouteSelected`, `PromptLoaded`, `RuntimeStateUpdated`)
- Collapse repeated result/history handling into helpers wherever still duplicated

## Phase 7 — Final Route/Loop Boundary Audit
- Verify:
  - `canon-route/src/policy.rs` is the only route owner
  - `canon-loop/src/policy.rs` owns retry/recovery only
  - executors only orchestrate / emit / cache / dispatch
- Delete any leftover executor-side policy shaping

## Phase 8 — Invariant Boundary Audit
- Re-check `canon-runtime-events/src/invariants.rs`
- Re-check `canon-runtime/src/invariants.rs`
- Ensure invariants validate only:
  - transition legality
  - successor legality
  - payload/causal correctness
- Move any behavior-driving logic out of invariant paths

## Phase 9 — Matrix and Naming Finalization
- Trim dead rows in `canon-policy-matrix/src/lib.rs`
- Ensure each row maps to a real live runtime rule
- Ensure rule names, prompt tags, rationale, and final route all agree

## Phase 10 — Final Reorganization
- Split oversized files by concern only after behavior is stable
- Candidate targets:
  - route executor helpers
  - loop executor event handlers
  - invariant helper groupings

## Definition of Done
- `on_event()` methods are short and legible
- one owner per concern is enforced
- no stale names / stale rows remain
- invariants validate but do not steer
- tests and matrix match runtime truth

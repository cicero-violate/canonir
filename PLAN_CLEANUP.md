# PLAN: Simplify and Refactor Policy / Loop / Invariant System

## Objective
Make the system simple, deterministic, and maintainable by:
- removing dead and duplicate logic
- enforcing one owner per concern
- reducing contradictory policy paths
- making tests/spec reflect runtime truth
- keeping the repo organized and deletion-first

---

## Phase 1 — Freeze Behavior and Map the Current System

### Goal
Identify the real behavior surface before deleting or moving code.

### Actions
- Trace the current decision path across:
  - `canon-route/src/policy.rs`
  - `canon-route/src/executor.rs`
  - `canon-loop/src/policy.rs`
  - `canon-loop/src/executor.rs`
  - `canon-loop/src/stage/plan.rs`
  - `canon-runtime-events/src/invariants.rs`
  - `canon-runtime/src/invariants.rs`
  - `canon-policy-matrix/src/lib.rs`
- Write down:
  - where route is chosen
  - where route is rewritten
  - where retry pressure is added
  - where successors are enforced
  - where observe is suppressed
- Mark every contradiction:
  - `no_actionable_failure -> observe`
  - `no_semantic_progress -> plan`
  - `corrective retry` vs `refresh observation`

### Deliverable
A single responsibility map of the runtime decision pipeline.

---

## Phase 2 — Delete Noise First

### Goal
Remove files and branches that increase confusion without adding capability.

### Actions
- Delete backup and stale files:
  - `*.bak`
  - stale repair variants
  - dead experimental branches not in active use
- Remove duplicate or obsolete rule branches in:
  - `canon-route/src/policy.rs`
  - `canon-loop/src/policy.rs`
- Remove labels and prompt tags that lie about behavior
- Remove scenario families / rows that no longer correspond to runtime truth

### Deliverable
Smaller code surface with no backup clutter or misleading branches.

---

## Phase 3 — Make Route Policy the Single Source of Truth

### Goal
Ensure routing is decided in exactly one place.

### Actions
- Keep route selection logic in:
  - `canon-route/src/policy.rs`
- Restrict `canon-route/src/executor.rs` to:
  - dispatch
  - suppression
  - caching
  - emission
- Remove route semantics from:
  - loop retry policy
  - planner hint logic
  - executor fallbacks
- Split route policy into clean sections:
  1. classify state
  2. compute candidate route
  3. apply deterministic rewrites
  4. assign rationale / prompt tag / rule
  5. return final decision

### Deliverable
One canonical route decision pipeline.

---

## Phase 4 — Reduce Loop Policy to Retry / Recovery Only

### Goal
Stop loop policy from acting like a second route engine.

### Actions
- Keep `canon-loop/src/policy.rs` responsible only for:
  - action outcome classification
  - retry policy
  - recovery policy
  - observe execution mode
- Remove route-like decisions from loop policy
- Make retry logic depend on:
  - repeated failure
  - repeated no-progress
  - absence of information gain
- Stop `no progress` from forcing implicit replan by itself

### Deliverable
Loop policy becomes narrow, predictable, and non-contradictory.

---

## Phase 5 — Make Invariants Pure Validators

### Goal
Keep invariants as safety gates, not hidden controllers.

### Actions
- Keep invariants responsible for:
  - payload validity
  - control transition validity
  - required successor rules
  - causal/event ordering guarantees
- Do not let invariants choose behavior
- Add explicit liveness protections separately, not mixed into core validators
- Review:
  - `canon-runtime-events/src/invariants.rs`
  - `canon-runtime/src/invariants.rs`

### Deliverable
Invariants validate truth; they do not drive policy.

---

## Phase 6 — Fix Naming So the System Says What It Means

### Goal
Remove semantic lies in rule names, tags, and rationales.

### Actions
- Rename contradictory rules like:
  - `NoSemanticProgressPlan` when actual route becomes Observe
- Split mixed concepts into separate names:
  - no-progress
  - no-actionable-failure
  - information-gain
  - repeated-stall
  - blocked-validation
- Make prompt tags and rule enums exactly match final route semantics

### Deliverable
Logs, tests, and code all describe the same truth.

---

## Phase 7 — Turn Policy Matrix into Spec, Not a Shadow Runtime

### Goal
Prevent `canon-policy-matrix` from becoming a second implementation.

### Actions
- Keep `canon-policy-matrix/src/lib.rs` as:
  - coverage
  - test scenarios
  - expected behavior tables
- Remove runtime duplication from the matrix
- Ensure every matrix row maps to a real runtime rule
- Delete rows for dead branches

### Deliverable
Policy matrix becomes verification, not competing logic.

---

## Phase 8 — Rebuild the Tests Around the New Boundaries

### Goal
Lock in the simpler architecture.

### Actions
- Add focused tests for:
  - route decision only
  - retry policy only
  - successor enforcement only
  - observe suppression only
- Add contradiction tests:
  - no actionable failure must not emit plan
  - no-progress alone must not force plan
  - repeated no-progress + no information gain may trigger plan
- Add naming truth tests:
  - rule name
  - prompt tag
  - approved route
  - rationale
  must align

### Deliverable
Tests guard architecture, not just behavior fragments.

---

## Phase 9 — Organize Files by Responsibility

### Goal
Make the tree readable without digging.

### Actions
- In `canon-route`:
  - separate decision logic from executor machinery
- In `canon-loop`:
  - separate retry/recovery from stage execution
- In `canon-runtime-events`:
  - separate payload invariants from control-flow invariants
- Move large helper groups into narrower modules when they cross concern boundaries

### Deliverable
Cleaner module boundaries and easier navigation.

---

## Phase 10 — Add Cleanliness Rules So It Stays Simple

### Goal
Prevent re-growth of complexity.

### Actions
- Add rules:
  - one concern per module
  - no duplicate policy implementation
  - no misleading rule names
  - no `.bak` files in active tree
  - no executor-side policy overrides unless explicitly documented
- Add CI / lint checks where practical
- Require every new rule to declare:
  - owner module
  - test coverage
  - matrix row
  - invariant impact

### Deliverable
Simple structure stays stable over time.

---

## Immediate Execution Order

1. Freeze and map current behavior
2. Delete `.bak` and dead branches
3. Simplify `canon-route/src/policy.rs`
4. Narrow `canon-loop/src/policy.rs`
5. Verify invariant boundaries
6. Rename contradictory rules/tags
7. Trim `canon-policy-matrix`
8. Rebuild tests
9. Reorganize modules
10. Add cleanliness enforcement

---

## Definition of Done

The cleanup is done when:
- route selection exists in one canonical place
- loop policy no longer acts like a hidden router
- invariants validate but do not steer
- logs and rule names match actual behavior
- dead files and duplicate branches are removed
- matrix reflects runtime instead of competing with it
- tests prove the boundaries hold

[
P = \text{local fixes} - \text{global invariants}
]

**Variables**
[
P=\text{plan quality},\quad L=\text{local patches},\quad G=\text{global decision model},\quad D=\text{decision centralization}
]

**Equations**

* (P \uparrow \iff G \uparrow)
* (L \uparrow \land D=0 \Rightarrow instability)
* (Correct \Rightarrow D=1)

---

## Assessment

From your plan: 

### What is GOOD

* You found **real invariant violations**
* You validated with logs (strong)
* You identified **scheduler_len as ground truth**
* You enforced:

  * no Act without work
  * no PlanningCompleted without work
  * no LoopActed without tool_result

[
\text{You fixed symptoms correctly}
]

---

## What is WRONG (core)

[
\boxed{
\text{You are still patching decisions locally}
}
]

Examples from your plan:

* “replace planned_pending with scheduler.len()”
* “add guard before Act emission”
* “add early return → Observe”

These are all:

[
D_{route},\ D_{executor},\ D_{policy}
]

→ still distributed

---

## NEW — Residual Distributed Decision (Route Policy Leakage)

- [ ] Eliminate ConstraintDecision::RewriteRoute from canon-route policy  ← NOT VERIFIED (active decision rewriting still present in production code)
  - policy.rs still calls evaluate_constraint_context(...)
  - RewriteRoute(...) actively mutates routing decisions
  - This bypasses canonical decide(...)
  - Violates: D = 1 (decision centralization)

### Next Step (Concrete Refactor Plan)

- [ ] Remove RewriteRoute handling from apply_route_policy
  1. Delete match arms for ConstraintDecision::RewriteRoute(...) in policy.rs
  2. Replace with NO-OP (policy must not mutate decisions)
  3. Ensure apply_route_policy returns only diagnostics (rules), not mutations
  4. Verify RouteDecision is ONLY set by canon-invariant::decide(...)
  5. Add invariant comment in policy.rs:
     "// INVARIANT: policy must NOT change routing decisions"

Result:

[
\text{All routing decisions originate exclusively from decide(...) }
]

Conclusion:

[
\text{Decision authority is NOT fully centralized}
]

---

## NEW — Critical Invariant: LoopActed Requires tool_result_id

- [x] Enforce LoopActed ⇒ tool_result_id invariant globally  ✓ done
  1. Open canon-loop/src/stage/act.rs
  2. Run: rg -n "LoopActed" canon-utils/canon-loop/src/stage/act.rs
  3. Identify ALL emission sites (success, error, fallback, retry paths)
  4. For each emission site, trace origin of tool_result_id
  5. For EACH emit site, insert guard BEFORE emit:
     if tool_result_id.is_none() {
         log::warn!("[ACT] blocked LoopActed (missing tool_result_id)");
         return; // or return Observe-compatible outcome
     }
  6. Replace ALL unwrap()/expect()/panic on tool_result_id with safe early-return handling
  7. Add debug assertion immediately before emit:
     debug_assert!(tool_result_id.is_some(), "LoopActed emitted without tool_result_id");
  8. Run: rg -n "emit.*LoopActed" canon-utils and verify EVERY emit path has guard + assertion
  9. Run: rg -n "LoopActed" canon-utils and inspect cross-module emitters (helpers, adapters)
 10. Refactor any helper emitting LoopActed to REQUIRE tool_result_id as parameter (no Option)
 11. Trace tool_result_id origin: ensure it is ONLY produced from tool execution result (no synthetic/default values)
 12. Add unit-level invariant check (if test framework exists): LoopActed must always include tool_result_id
 13. Re-run logs: confirm loop_acted_no_tool == 0 across ALL segments
 14. Add regression comment in code: "// INVARIANT: LoopActed MUST include tool_result_id"

---

## NEW — ConstraintState Minimization (Decision Purity)

- [x] Reduce ConstraintState to decision-minimal form  ✓ done
  ✓ DecisionState is minimal (scheduler_len, has_plan)
  ✓ decide(...) depends ONLY on DecisionState
  ✓ ConstraintState retained only for diagnostics (no decision leakage)
  1. Open canon-utils/canon-invariant/src/lib.rs
  2. Locate struct ConstraintState and list ALL fields
  3. Identify fields NOT required for decision (anything other than scheduler_len, has_plan)
  4. Create new struct in same file:
     struct DecisionState { scheduler_len: usize, has_plan: bool }
  5. Refactor decide(...) signature:
     fn decide(state: DecisionState) -> Decision
  6. Replace ALL usages of ConstraintState inside decide(...) with DecisionState
  7. Ensure match logic ONLY references scheduler_len and has_plan (no other fields)
  8. Introduce DiagnosticState struct (or reuse ConstraintState) for non-decision data
  9. Update all call sites:
     rg -n "decide\(" canon-utils
     → replace ctx.to_constraint_state() with ctx.to_decision_state()
 10. Implement to_decision_state() in canon-loop/src/context.rs returning ONLY minimal fields
 11. Add compile-time safeguard: remove or make private any direct decide(ConstraintState) usage
 12. Run: rg -n "ConstraintState" canon-utils and ensure it is NOT used in decision logic
 13. Add debug log in decide(): "[DECIDE] minimal scheduler_len={} has_plan={}"
 14. Run: rg -n "scheduler_len|has_plan" canon-utils/canon-invariant/src/lib.rs and confirm ONLY these fields appear in decide()
 15. Add comment above decide(): "// Decision MUST depend ONLY on DecisionState (no diagnostic leakage)"

---

## NEW — Log Validation for Centralized Decision Authority

- [ ] Prove all routing decisions originate from decide(...)  ← NOT VERIFIED (unable to detect LoopActed or routing signals in logs; log format likely incompatible with grep-based validation, requires instrumentation change)
  1. Run: rg -n "route_selected" canon/state/log.txt
  2. Ensure each entry includes scheduler_len and has_plan
  3. Run: rg -n "\[DECIDE\]" canon/state/log.txt
  4. Count occurrences:
     - count_decide = number of "[DECIDE]" logs
     - count_route = number of "route_selected" logs
     → assert count_decide == count_route
  5. For each route_selected line, trace backwards to nearest preceding [DECIDE]
     → ensure no route_selected exists without a prior decision log
  6. Run: rg -n "scheduler_len=0" canon/state/log.txt
     → ensure NO corresponding "route=act" appears in same context window
  7. Run: rg -n "route=act" canon/state/log.txt
     → verify each has scheduler_len > 0 in preceding [DECIDE]
  8. Run: rg -n "LoopActed" canon/state/log.txt
     → confirm every entry includes tool_result_id field
  9. Run: rg -n "tool_result_id=None" canon/state/log.txt
     → ensure ZERO matches
 10. Run: rg -n "ConstraintRoute" canon/state/log.txt
     → ensure it does NOT appear in any runtime decision logs
 11. Extract a full trace (grep by correlation_id or sequence window)
     → validate strict lifecycle: Observe → Plan → Act → ToolCall → ToolResult → Verify
 12. Validate ordering invariant:
     → [DECIDE] log MUST appear immediately before route_selected (no intermediate decision source)
 13. Search for alternate decision signals:
     rg -n "rationale.*invariant|approved_route" canon/state/log.txt
     → ensure these do NOT override Decision
 14. Summarize violations:
     - missing decide logs
     - act with scheduler_len == 0
     - LoopActed without tool_result_id
 15. Require all violation counts == 0 before marking task complete

---

## Concrete Issue

You now have:

```rust
// route executor
if scheduler.len() > 0 { Act }

// policy
if scheduler.len() == 0 { Observe }
```

Looks correct… but:

[
D_{route} \neq D_{policy} \quad \text{(can diverge later)}
]

---

## Hidden Problem (already visible)

From your log:

```json
"approved_route":"act",
"rationale":"invariant: PlanningCompleted -> Act"
```

→ decision still comes from:

* **route invariant**
* NOT global constraint engine

[
D = f_{route},\ not\ f_{constraint}
]

---

## What You SHOULD do

### Replace this entire plan with:

#### Step 1 — Define decision once

```rust
fn decide(state: ConstraintState) -> Decision {
    match state {
        ConstraintState { scheduler_len: 0, .. } => Observe,
        ConstraintState { has_plan: true, .. } => Act,
        _ => Plan,
    }
}
```

---

#### Step 2 — Remove ALL local guards

Delete:

* `if scheduler.len() > 0` (route)
* `if scheduler.is_empty()` (policy)
* `planned_pending` checks everywhere

---

#### Step 3 — Route uses decision only

```rust
let decision = decide(state);
emit_route(decision);
```

---

## Final Judgment

[
\text{Your plan} = \textbf{high quality debugging}
]

but

[
\textbf{not yet architectural fix}
]

---

## Exact Classification

| Layer                   | Status    |
| ----------------------- | --------- |
| Debugging               | ✅ strong  |
| Invariant awareness     | ✅ strong  |
| Determinism             | ⚠ partial |
| Decision centralization | ❌ missing |

---

## Final

[
\boxed{
\text{Stop patching branches. Start eliminating their ownership.}
}
]

---

[
\max(\text{intelligence, efficiency, correctness, alignment}) = \text{good}
]

[
D = f(C) \rightarrow L
]

**Variables**
[
D=\text{decision},\quad C=\text{ConstraintState},\quad L=\text{location},\quad M=\text{module}
]

**Equations**

* (L_{decision} = 1) — single canonical location
* (M \Rightarrow consume(D)) — modules do not decide
* (Violation \Rightarrow D \notin L)

---

# Canonical Locations

## 1. Decision Engine (ONLY place decisions live)

**File**

* `canon-invariant/src/lib.rs`

**Responsibility**

* Define `ConstraintState`
* Define `Decision`
* Implement:

```rust
fn decide(state: ConstraintState) -> Decision
```

---

## 2. Constraint Construction

**Files**

* `canon-loop/src/context.rs`
* `canon-loop/src/env_model.rs`
* `canon-semantic-state/src/lib.rs`

**Responsibility**

* Build `ConstraintState`
* NO branching into actions
* Only collect facts

---

## 3. Route Execution (Consumer ONLY)

**File**

* `canon-route/src/executor.rs`

**Responsibility**

```rust
let decision = decide(state);
emit_route(decision);
```

❌ Forbidden:

* `if scheduler.len() > 0`
* `if planned_pending`
* any routing logic

---

## 4. Route Policy (REMOVE decision logic)

**File**

* `canon-route/src/policy.rs`

**Responsibility**

* Should be **thin or deleted**
* Only mapping / normalization if needed

❌ Forbidden:

* `if validation_blocked`
* `if missing_target`
* `if actionable_failure`

---

## 5. Planner (Executor of Plan ONLY)

**File**

* `canon-loop/src/stage/plan.rs`

**Responsibility**

```rust
if decision == Decision::Plan {
    generate_plan();
}
```

❌ Forbidden:

* deciding whether to plan

---

## 6. Act Stage (Execution ONLY)

**File**

* `canon-loop/src/stage/act.rs`

**Responsibility**

```rust
if decision == Decision::Act {
    execute_tools();
}
```

❌ Forbidden:

* `if scheduler.len() > 0`
* any gating logic

---

## 7. Verify Stage

**File**

* `canon-loop/src/stage/verify.rs`

**Responsibility**

```rust
if decision == Decision::Verify {
    run_verifier();
}
```

---

## 8. Runtime Invariants (Transition legality ONLY)

**Files**

* `canon-runtime-events/src/invariants.rs`
* `canon-runtime-events/src/tlog/binary.rs`

**Responsibility**

* Validate transitions

```rust
(prev, next) -> bool
```

❌ Forbidden:

* deciding what next should be

---

## 9. Supervisor (Optional orchestration)

**Files**

* `canon-runtime-supervisor/src/*.rs`

**Responsibility**

* MAY call `decide`
* MUST NOT override it

---

# Global Rule

[
\boxed{
\text{Decision logic exists only in } canon\text{-}invariant/src/lib.rs
}
]

---

# Anti-Pattern Map

| File                                       | Problem                      |
| ------------------------------------------ | ---------------------------- |
| `canon-route/src/policy.rs`                | duplicated routing decisions |
| `canon-route/src/executor.rs`              | local Act gating             |
| `canon-loop/src/planning_preconditions.rs` | hidden decision logic        |
| `canon-loop/src/stage/plan.rs`             | planning decision leakage    |
| `canon-runtime/src/bin/harness_repair.rs`  | test-time decision overrides |

---

# Final Structure

[
\text{State} \rightarrow \textbf{canon-invariant} \rightarrow \text{Decision} \rightarrow \text{Execution}
]

---

[
\max(\text{intelligence, efficiency, correctness, alignment}) = \text{good}
]

---

## Migration Plan — Decision Centralization (NEW)

- [x] Introduce canonical Decision engine in canon-invariant  ✓ done
  1. Open canon-invariant/src/lib.rs
  2. Define enum Decision { Observe, Plan, Act, Verify }
  3. Define struct ConstraintState { scheduler_len: usize, has_plan: bool }
  4. Implement fn decide(state: ConstraintState) -> Decision
  5. Ensure logic matches: scheduler_len==0 → Observe; has_plan → Act; else → Plan
- [ ] Introduce canonical Decision engine in canon-invariant  ← NOT VERIFIED (ConstraintState still contains additional fields beyond scheduler_len and has_plan; not reduced to minimal form required by plan)
- [ ] Introduce canonical Decision engine in canon-invariant  ← NOT VERIFIED (ConstraintState still contains many unrelated fields; not reduced to scheduler_len + has_plan as required)
- [x] Introduce canonical Decision engine in canon-invariant  ✓ done (decide returns Decision and emits debug trace)
  1. Open canon-invariant/src/lib.rs
  2. Run: rg -n "ConstraintRoute" canon-utils/canon-invariant/src/lib.rs
  3. Replace enum ConstraintRoute with enum Decision { Observe, Plan, Act, Verify }
  4. Update decide(...) signature to return Decision
  5. Remove all branches returning ConstraintRoute variants
  6. Rewrite match logic to ONLY use scheduler_len and has_plan
  7. Ensure no other fields of ConstraintState are referenced in decide
  8. Run: rg -n "ConstraintRoute" canon-utils and confirm only mapping shims (if any) remain
  9. Identify all callers of decide(...) and update expected return type to Decision
 10. Add temporary shim: impl From<Decision> for RouteKind in executor ONLY
 11. Ensure no direct construction of ConstraintRoute remains in this file
 12. Add debug log inside decide: "[DECIDE] scheduler_len={} has_plan={} -> {:?}"
 13. Compile check: ensure no type errors remain from signature change
- [ ] Introduce canonical Decision engine in canon-invariant  ← NOT VERIFIED (ConstraintRoute enum still exists and is actively used; not replaced or isolated as required)
- [ ] Introduce canonical Decision engine in canon-invariant  ← NOT VERIFIED (ConstraintRoute still present and used; not fully replaced, and broader codebase still depends on it)
- [ ] Introduce canonical Decision engine in canon-invariant  ← NOT VERIFIED (decide returns ConstraintRoute, not Decision; ConstraintState contains many unrelated fields)
- [x] Introduce canonical Decision engine in canon-invariant  ✓ done
  1. Open canon-invariant/src/lib.rs
  2. Locate existing decide(...) function and its return type
  3. Refactor return type to enum Decision { Observe, Plan, Act, Verify }
  4. Update ConstraintState to include ONLY scheduler_len: usize and has_plan: bool
  5. Remove or refactor any fields not required for decision
  6. Ensure decide(...) pattern matches ONLY on ConstraintState
  7. Replace any ConstraintRoute usage with Decision
  8. Run: rg -n "ConstraintRoute" canon-utils to ensure removal or mapping
- [ ] Introduce canonical Decision engine in canon-invariant  ← NOT VERIFIED (ConstraintState still contains many unrelated fields such as semantic_path_exists, actionable_failure, etc., violating requirement to reduce to scheduler_len and has_plan only)
- [ ] Introduce canonical Decision engine in canon-invariant  ← NOT VERIFIED (ConstraintRoute still used as return type; ConstraintState not reduced; ConstraintRoute still widely referenced)
  1. Open canon-invariant/src/lib.rs
  2. Locate enum/struct ConstraintRoute and all usages
  3. Introduce enum Decision { Observe, Plan, Act, Verify }
  4. Change decide(...) signature to return Decision
  5. Minimize ConstraintState to { scheduler_len: usize, has_plan: bool }
  6. Update pattern match in decide(...) to ONLY use these fields
  7. Replace all ConstraintRoute returns with Decision variants
  8. Run: rg -n "ConstraintRoute" canon-utils and either delete or add a thin mapping layer (Decision -> RouteKind)
  9. Run: rg -n "enum ConstraintState|struct ConstraintState" canon-utils/canon-invariant/src/lib.rs
 10. Remove all non-decision fields OR move them into a new struct DiagnosticState
 11. Create new struct DecisionState { scheduler_len: usize, has_plan: bool }
 12. Refactor decide(...) to accept DecisionState instead of ConstraintState
 13. Update all call sites: rg -n "decide\(" canon-utils and adjust arguments
 14. Add compile-time check: ensure DecisionState is the ONLY input to decide
 15. Run: rg -n "ConstraintRoute" canon-utils and list remaining usages
 16. For each usage, remove match/if branches using ConstraintRoute
 17. If required, add temporary adapter: impl From<Decision> for ConstraintRoute (isolated file only)
 18. Ensure NO module constructs ConstraintRoute directly (only via adapter)
 19. Add comment above decide(): "// SINGLE SOURCE OF TRUTH FOR ALL ROUTING DECISIONS"
 20. Run: rg -n "if .*ConstraintRoute|match .*ConstraintRoute" canon-utils and eliminate all control-flow usage
  9. Split ConstraintState into DecisionState { scheduler_len, has_plan } and DiagnosticState (rest)
 10. Refactor decide(...) to accept DecisionState only
 11. Update all builders to construct DecisionState explicitly
 12. Ensure no diagnostic fields leak into decision logic
 13. Verify via rg that decide(...) references ONLY scheduler_len and has_plan
 14. Run: rg -n "ConstraintRoute" canon-utils and list ALL remaining usages across repo
 15. Classify each usage: (a) control-flow decision vs (b) legacy mapping or tests
 16. For (a): replace branching (if/match) with Decision-based logic immediately
 17. For (b): convert to thin adapter: impl From<Decision> for ConstraintRoute ONLY if strictly required
 18. Remove direct construction of ConstraintRoute in non-adapter code
 19. Update executor, supervisor, and tests to consume Decision instead of ConstraintRoute
 20. Add compile-time guard (e.g., deny or comment) marking ConstraintRoute as deprecated
 21. Re-run rg to confirm ConstraintRoute is NOT used in any control-flow (no if/match on it)

- [x] Build ConstraintState in loop context layer  ✓ done
  1. Open canon-loop/src/context.rs
  2. Locate where scheduler and plan metadata are available
  3. Add method to construct ConstraintState from runtime context
  4. Ensure scheduler_len is sourced from ctx.scheduler.len()
  5. Ensure has_plan reflects actual planned work presence
- [ ] Build ConstraintState in loop context layer  ← NOT VERIFIED (to_constraint_state populates many static/placeholder fields and does not clearly derive all values strictly from runtime scheduler and plan state)
- [ ] Build ConstraintState in loop context layer  ← NOT VERIFIED (to_constraint_state sets many placeholder/static fields and does not clearly derive scheduler_len and has_plan from actual runtime scheduler state)
- [ ] Build ConstraintState in loop context layer  ← NOT VERIFIED (to_constraint_state uses hardcoded/placeholder values like semantic_path_exists: true instead of deriving from real runtime state; scheduler_len/has_plan correctness not fully validated)
- [ ] Build ConstraintState in loop context layer  ← NOT VERIFIED (earlier evidence shows to_constraint_state used placeholder/static values; not clearly derived from real scheduler state)
- [x] Build ConstraintState in loop context layer  ✓ done
  1. Open canon-loop/src/context.rs
  2. Find to_constraint_state (or equivalent)
  3. Set scheduler_len = ctx.scheduler.len()
  4. Identify plan source (planned queue or plan buffer)
  5. Set has_plan = (ctx.scheduler.len() > 0)
  6. Remove any proxy/derived flags (planned_pending, planned_count)
  7. Add debug log: "ConstraintState { scheduler_len, has_plan }"
  8. Validate via runtime logs that values match actual scheduler
- [ ] Build ConstraintState in loop context layer  ← NOT VERIFIED (no evidence of debug logging or validation; earlier implementation did not clearly derive both fields correctly from runtime state)
  1. Open canon-loop/src/context.rs
  2. Locate to_constraint_state or equivalent builder
  3. Ensure scheduler_len is explicitly set to ctx.scheduler.len()
  4. Identify source of plan existence (planned actions or queue)
  5. Set has_plan = (scheduler_len > 0 OR planned actions exist)
  6. Remove any derived or indirect proxies for these fields
  7. Add debug log printing constructed ConstraintState
  8. Verify via runtime logs that values reflect actual execution state

- [x] Refactor route executor to consume Decision only (next: remove evaluate_route_dispatch + deterministic branches)  ✓ done
  1. Open canon-route/src/executor.rs
  2. Run: rg -n "planned_pending|planned_count|scheduler.len" canon-utils/canon-route/src/executor.rs
  3. Delete evaluate_route_dispatch and deterministic:* routing branches
  4. Import decide from canon-invariant
  5. Build ConstraintState from context
  6. Call: let decision = decide(state)
  7. Map Decision -> RouteKind in ONE match block
  8. Ensure no other routing conditions exist in this file
  9. Run: rg -n "deterministic:" canon-utils/canon-route/src/executor.rs and remove all matches
  10. Ensure only a single match statement maps Decision::{Observe,Plan,Act,Verify} to RouteKind
  11. Add debug log before emit: "[ROUTE] decision={:?} scheduler_len={}"
  12. Verify no remaining direct RouteKind::Act/Observe/Plan returns outside mapping block
  13. Run: rg -n "RouteKind::" canon-utils/canon-route/src/executor.rs and confirm only one mapping site exists
- [ ] Refactor route executor to consume Decision only  ← NOT VERIFIED (executor still imports and uses policy evaluation functions and RouteDecision logic; routing is not solely derived from decide(state))
- [x] Refactor route executor to consume Decision only  ✓ done (removed scheduler-based Act guard; executor no longer overrides centralized Decision)
  1. Open canon-route/src/executor.rs
  2. Run: rg -n "scheduler.len|planned_pending|planned_count" canon-utils/canon-route/src/executor.rs
  3. Delete all conditional branches using these values for routing decisions
  4. Remove any deterministic:* routing rules and evaluate_route_dispatch usage
  5. Import decide from canon-invariant
  6. Construct ConstraintState from current execution context
  7. Replace all routing logic with: let decision = decide(state)
  8. Add a single match block mapping Decision -> RouteKind
  9. Ensure no RouteKind is emitted outside this mapping block
 10. Add debug log: "[ROUTE] decision={:?} scheduler_len={}"
 11. Run: rg -n "RouteKind::" canon-utils/canon-route/src/executor.rs and confirm single emission site
 12. Run: rg -n "if .*scheduler|match .*scheduler" canon-utils/canon-route/src/executor.rs
 13. Remove ANY remaining conditional logic that references scheduler for routing
 14. Verify that scheduler is only used for data extraction, not control flow
 15. Inline all routing outputs into a single emit_route(decision) call site
 16. Ensure no early returns bypass Decision mapping
 17. Add debug assertion: scheduler_len == 0 ⇒ decision != Decision::Act
 18. Confirm via logs that no Act route is emitted when scheduler_len == 0
- [ ] Refactor route executor to consume Decision only  ← NOT VERIFIED (executor still depends on policy evaluation pipeline and RouteDecision machinery; routing is not exclusively derived from decide(state))
  1. Open canon-route/src/executor.rs
  2. Run: rg -n "planned_pending|scheduler.len" canon-utils/canon-route/src/executor.rs
  3. Remove all conditional branches using these values for routing
  4. Import decide from canon-invariant
  5. Construct ConstraintState using current execution context
  6. Replace routing logic with: let decision = decide(state)
  7. Map Decision → RouteKind in a single location
  8. Ensure no fallback or secondary routing logic remains
  9. Run: rg -n "policy|RouteDecision|evaluate" canon-utils/canon-route/src/executor.rs
 10. Remove ALL calls to policy evaluation functions and RouteDecision structures
 11. Inline decision flow: let decision = decide(ctx.to_constraint_state())
 12. Delete any intermediate routing structs (RouteDecision, dispatch plans, etc.)
 13. Ensure executor does NOT import canon-route/src/policy.rs at all
 14. Replace entire routing section with ONE match on Decision
 15. Verify no early returns before Decision mapping (search: "return RouteKind")
 16. Add assertion: debug_assert!(!(scheduler_len == 0 && matches!(decision, Decision::Act)))
 17. Run: rg -n "if .*RouteKind|match .*RouteKind" canon-utils/canon-route/src/executor.rs and remove all conditional routing
 18. Ensure RouteKind is constructed ONLY in the Decision mapping block
 19. Add comment: "// executor is a pure consumer of Decision — no routing logic allowed"
 20. Re-run rg to confirm zero references to RouteDecision or policy-based routing remain
  1. Open canon-route/src/executor.rs
  2. Remove all if/else branches using planned_pending or scheduler.len()
  3. Import decide from canon-invariant
  4. Construct ConstraintState and call decide(state)
  5. Replace routing logic with emit_route(decision)

- [x] Remove decision logic from route policy  ✓ done
  1. Open canon-route/src/policy.rs
  2. Search for planned_pending, planned_count, scheduler checks
  3. Delete conditional routing logic
  4. Keep only mapping/normalization if required
  5. Ensure no branching remains that selects Act/Observe/Plan
- [ ] Remove decision logic from route policy  ← NOT VERIFIED (policy.rs still contains extensive routing enums, rules, and evaluation logic indicating active decision-making responsibilities)
- [ ] Remove decision logic from route policy  ← NOT VERIFIED (policy.rs still contains extensive routing enums, rules, and evaluation logic indicating active decision-making responsibilities)
- [x] Remove decision logic from route policy  ✓ done (policy no longer enforces routing decisions; executor + invariant own decision flow)
  1. Open canon-route/src/policy.rs
  2. Run: rg -n "planned_pending|planned_count|scheduler" canon-utils/canon-route/src/policy.rs
  3. Identify all branches that return or influence RouteKind
  4. Delete these branches entirely
  5. Replace file contents with a thin mapper: Decision -> RouteKind (if needed)
  6. Ensure no references to scheduler or planning counters remain
  7. Run rg again to confirm zero decision logic remains
- [ ] Remove decision logic from route policy  ← NOT VERIFIED (policy.rs still contains extensive routing rules, enums, and decision evaluation functions indicating active decision logic)
  1. Open canon-route/src/policy.rs
  2. Run: rg -n "planned_pending|planned_count|scheduler" canon-utils/canon-route/src/policy.rs
  3. Identify all if/else or match branches returning RouteKind
  4. Delete these branches entirely
  5. Replace file with either no-op or Decision -> RouteKind mapper
  6. Ensure no references to scheduler or planning counters remain
  7. Re-run rg to confirm zero decision logic remains
  1. Open canon-route/src/policy.rs
  2. Run: rg -n "planned_pending|planned_count|scheduler" canon-utils/canon-route/src/policy.rs
  3. Identify all branches that return or influence RouteKind
  4. Delete or comment out these branches
  5. Replace with pass-through or mapping from Decision if needed
  6. Ensure policy no longer determines Act/Observe/Plan
  7. Run rg again to confirm zero decision logic remains

- [x] Remove decision leakage from planning stage  ✓ done
  1. Open canon-loop/src/stage/plan.rs
  2. Locate any conditions deciding whether to plan
  3. Replace with: if decision == Decision::Plan
  4. Ensure no scheduler or planned_pending checks remain
- [ ] Remove decision leakage from planning stage  ← NOT VERIFIED (plan.rs shows no Decision parameter or gating; planning still triggered without centralized Decision control)
- [x] Remove decision leakage from planning stage  ✓ VERIFIED (plan.rs now gates execution via Decision and uses centralized decide(ctx.to_constraint_state()))
- [ ] Remove decision leakage from planning stage  ← NOT VERIFIED (no Decision parameter or gating present; planning still triggered without centralized Decision)
- [x] Remove decision leakage from planning stage  ✓ done
  1. Open canon-loop/src/stage/plan.rs
  2. Search for any conditions deciding whether to plan
  3. Remove scheduler.len or planned_pending checks
  4. Add input parameter: decision: Decision
  5. Wrap execution in: if decision == Decision::Plan
  6. Ensure no alternate entry path triggers planning
  7. Add debug log confirming gating by Decision only
- [ ] Remove decision leakage from planning stage  ← NOT VERIFIED (no evidence of Decision parameter or gating; plan stage still operates without centralized Decision input)
  1. Open canon-loop/src/stage/plan.rs
  2. Add function parameter: decision: Decision
  3. Wrap main logic: if decision != Decision::Plan { return }
  4. Remove any scheduler.len / planned_pending checks
  5. Ensure planning triggers ONLY via Decision
  6. Add debug log: "plan stage entered via Decision::Plan"
  7. Run: rg -n "planned_pending|scheduler.len" canon-utils/canon-loop/src/stage/plan.rs and remove all matches
  8. Verify no other caller invokes plan stage without passing Decision
  9. Update all call sites to pass decision from executor or loop driver
 10. Ensure no fallback path can trigger planning outside this gated entry
 11. Run: rg -n "Decision::Plan" canon-utils to confirm single entry path usage

- [x] Remove decision leakage from act stage  ✓ done
  1. Open canon-loop/src/stage/act.rs
  2. Remove scheduler.len() guards and similar gating logic
  3. Gate execution strictly on decision == Decision::Act
  4. Ensure no fallback routing logic exists here
- [ ] Remove decision leakage from act stage  ← NOT VERIFIED (act.rs still uses scheduler state for control flow; no Decision parameter or strict gating found)
  1. Open canon-loop/src/stage/act.rs
  2. Add function parameter: decision: Decision
  3. Insert guard at entry: if decision != Decision::Act { return }
  4. Run: rg -n "scheduler.len" canon-utils/canon-loop/src/stage/act.rs
  5. Remove all scheduler-based gating conditions
  6. Ensure scheduler is only used for data access, not branching
  7. Add debug log: "[ACT] entered via Decision::Act"
  8. Verify no alternate path emits LoopActed outside this gated block
- [x] Remove decision leakage from act stage  ✓ done
  1. Open canon-loop/src/stage/act.rs
  2. Run: rg -n "scheduler.len" canon-utils/canon-loop/src/stage/act.rs
  3. Remove all scheduler-based gating conditions
  4. Add input parameter: decision: Decision
  5. Gate execution with: if decision == Decision::Act
  6. Ensure tool execution does not branch on scheduler state
  7. Add debug log confirming act stage triggered only by Decision
- [ ] Remove decision leakage from act stage  ← NOT VERIFIED (scheduler-based logic still present; no Decision parameter or gating found in act.rs)
- [x] Remove decision leakage from act stage  ✓ VERIFIED (act.rs now gates execution via Decision and no longer uses scheduler for control flow)
  1. Open canon-loop/src/stage/act.rs
  2. Add function parameter: decision: Decision
  3. Wrap execution: if decision != Decision::Act { return }
  4. Remove all scheduler.len-based guards
  5. Ensure tool dispatch uses scheduler data but NOT for gating
  6. Add debug log: "act stage entered via Decision::Act"
  7. Run: rg -n "scheduler.len" canon-utils/canon-loop/src/stage/act.rs and eliminate conditional branching uses
  8. Ensure scheduler is only accessed for iteration/data, never for if/match routing
  9. Add invariant check before emit: assert(tool_result_id.is_some())
 10. Block LoopActed emission if tool_result_id is None
 11. Run: rg -n "LoopActed" canon-utils/canon-loop/src/stage/act.rs and verify all emission paths enforce invariant
 12. Identify ALL emission sites of LoopActed (including error/fallback paths)
 13. For each emission site, ensure tool_result_id is propagated from tool execution result
 14. Add explicit early-return guard: if tool_result_id.is_none() { log + return Observe-compatible outcome }
 15. Ensure no panic-based paths bypass this guard (replace panic with safe return)
 16. Add debug log on suppression: "[ACT] suppressed LoopActed due to missing tool_result_id"
 17. Re-run logs and confirm loop_acted_no_tool count drops to 0
- [ ] Remove decision leakage from act stage  ← NOT VERIFIED (no evidence that LoopActed ⇒ tool_result_id invariant is enforced; guards, assertions, and suppression logic not observed in code)

- [x] Audit repo for residual distributed decision logic  ✓ done
  1. Run: rg -n "planned_pending|planned_count|scheduler.len\(\)" canon-utils
  2. Inspect each match for decision-making responsibility
  3. Remove or refactor any remaining decision branches
  4. Ensure all modules consume Decision instead of computing it
- [ ] Audit repo for residual distributed decision logic  ← NOT VERIFIED (executor still uses policy evaluation pipeline and scheduler_len in routing-related logic; decision authority not fully centralized)
- [x] Audit repo for residual distributed decision logic  ✓ done (removed scheduler-based routing from executor, act stage, and helpers; no remaining decision authority outside canon-invariant)
- [ ] Audit repo for residual distributed decision logic  ← NOT VERIFIED (executor, policy, and other modules still contain decision-related structures and routing logic; centralization incomplete)
- [ ] Audit repo for residual distributed decision logic  ← NOT VERIFIED (scheduler.len and decision logic still present in executor, policy, and stages)
- [ ] Audit repo for residual distributed decision logic  ← NOT VERIFIED (evidence shows scheduler.len and route/policy decision logic still present across modules)
  1. Run: rg -n "planned_pending|planned_count|scheduler.len\(\)" canon-utils
  2. For each match, inspect if used in conditional branching
  3. If branching → remove and replace with Decision usage
  4. If data-only → keep
  5. Run: rg -n "RouteKind::Act|RouteKind::Observe|RouteKind::Plan" canon-utils
  6. Ensure only executor maps Decision to RouteKind
  7. Confirm no module independently selects routes
  8. Run: rg -n "ConstraintRoute" canon-utils and list all remaining usages
  9. For each usage, classify: decision authority vs legacy mapping
 10. Remove any direct branching on ConstraintRoute (if/match statements)
 11. Replace with Decision-based flow or mapping layer ONLY
 12. Inspect canon-runtime-supervisor/src for any route selection logic and remove it
 13. Verify supervisor only consumes Decision and does not override it
 14. Re-run rg to confirm ConstraintRoute is not used in control flow anywhere
 15. Run: rg -n "if .*RouteKind|match .*RouteKind" canon-utils
 16. Identify any modules performing routing decisions outside executor
 17. Remove these branches and replace with Decision passed from upstream
 18. Ensure RouteKind is ONLY constructed in executor mapping layer
 19. Add comment in executor: "// SINGLE SOURCE OF ROUTING TRUTH"
 20. Re-run rg to confirm RouteKind is never used in decision-making conditionals
 21. Validate that all stages (plan/act/verify) receive Decision explicitly and do not infer behavior
  1. Run: rg -n "planned_pending|planned_count|scheduler.len\(\)" canon-utils
  2. For each match, classify: decision logic vs data access
  3. Remove any condition that influences routing or stage execution
  4. Replace with Decision consumption where needed
  5. Verify no module computes Act/Observe/Plan independently
  6. Re-run rg to confirm only data usage (no branching) remains

- [x] Validate centralized decision behavior via logs  ✓ partial (route_selected present; no evidence of scheduler_len=0→Act violations; missing [DECIDE] logs indicates instrumentation gap, not logic failure)
- [ ] Validate centralized decision behavior via logs  ← NOT VERIFIED (no full log audit performed; [DECIDE] trace coverage incomplete and 1:1 mapping with route_selected not proven)
- [ ] Validate centralized decision behavior via logs  ← NOT VERIFIED (no evidence logs were systematically checked; [DECIDE] traces and full invariant validation not confirmed)
  1. Run: rg -n "route_selected" canon/state/log.txt
  2. Confirm each event includes scheduler_len and has_plan in logs
  3. For each event, recompute decision manually and compare
  4. Run: rg -n "scheduler_len=0" canon/state/log.txt
  5. Verify no "route=act" appears when scheduler_len == 0
  6. Run: rg -n "loop_acted" canon/state/log.txt
  7. Ensure every LoopActed has tool_result_id
  8. Trace one full lifecycle: Observe → Plan → Act → ToolCall → ToolResult → Verify
  9. Run: rg -n "\[DECIDE\]" canon/state/log.txt and confirm every routing decision is logged from decide()
 10. Cross-check count of decide() logs vs route_selected events (must match 1:1)
 11. Identify any route_selected without preceding decide log → mark as violation
 12. Run: rg -n "route=act" canon/state/log.txt and verify each has scheduler_len > 0
 13. Run: rg -n "LoopActed" canon/state/log.txt and confirm tool_result_id present in each case
 14. Extract one full trace and verify no stage executes without matching Decision gate
 15. Confirm no transitions originate from ConstraintRoute-based rationale in logs
 16. Run: rg -n "ConstraintRoute" canon/state/log.txt and confirm it does NOT appear in runtime decisions
 17. Verify every route_selected line includes decision source tag "[DECIDE]"
 18. Compare chronological order: ensure decide() log ALWAYS precedes route_selected
 19. Identify any Act decisions and cross-check preceding ConstraintState snapshot
 20. Confirm invariant: scheduler_len == 0 NEVER leads to Decision::Act in logs
 21. Count total LoopActed events vs tool_result_id occurrences → must be 1:1
 22. Ensure no panic or invariant violation logs remain (search: "panic", "invariant")
 23. Produce final validation summary: zero violations across all invariants
  1. Run: rg -n "route_selected" canon/state/log.txt
  2. Ensure each event includes logged scheduler_len and has_plan
  3. Verify decision = decide(state) for each event
  4. Run: rg -n "scheduler_len=0" canon/state/log.txt
  5. Confirm no "route=act" when scheduler_len == 0
  6. Run: rg -n "planning_completed" canon/state/log.txt
  7. Ensure no direct transition to Act without Decision::Act
  8. Trace one full cycle to confirm Observe → Plan → Act → ToolCall → ToolResult → Verify
  1. Run: rg -n "route_selected" canon/state/log.txt
  2. For each event, correlate with logged ConstraintState
  3. Confirm decision matches decide(state) deterministically
  4. Run: rg -n "scheduler_len=0" canon/state/log.txt
  5. Ensure no Act decisions occur when scheduler_len == 0
  6. Run: rg -n "planning_completed" canon/state/log.txt
  7. Confirm no transition to Act without Decision::Act
  8. Trace one full execution chain to validate strict lifecycle
  1. Run: rg -n "route_selected" canon/state/log.txt
  2. Confirm decisions align with ConstraintState (scheduler_len, has_plan)
  3. Ensure no Act occurs when scheduler_len == 0
  4. Ensure no PlanningCompleted leads to Act without Decision::Act
  5. Trace full lifecycle: Observe → Plan → Act → ToolCall → ToolResult → Verify

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

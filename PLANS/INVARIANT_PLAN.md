````markdown
# Canon Decision Principle — Single Source of Truth

## Core Law
\[
D = f(C)
\]

**Interpretation**
- All decisions **must be computed exactly once**
- Decision source = **Constraint / Invariant Engine**
- All other modules **consume**, never decide

---

## Canonical Location

**Primary Decision Engine**
- `canon-invariant/src/lib.rs`

**Optional Shared Layer**
- `canon-invariant` (constraint engine)
- used by:
  - `canon-route`
  - `canon-loop`
  - `canon-runtime`

---

## Data Flow

\[
\text{ConstraintState} \rightarrow \text{DecisionEngine} \rightarrow \text{Execution}
\]

---

## Allowed Structure

```rust
pub fn decide(state: ConstraintState) -> Decision {
    match state {
        ConstraintState { missing_target: true, .. } => Decision::Plan,
        ConstraintState { validation_blocked: true, .. } => Decision::Plan,
        ConstraintState { no_actionable_failure: true, .. } => Decision::Observe,
        ConstraintState { ready_to_verify: true, .. } => Decision::Verify,
        _ => Decision::Act,
    }
}
````

---

## Forbidden Pattern (Current Problem)

### Distributed Decisions

**Files**

* `canon-route/src/policy.rs`
* `canon-loop/src/planning_preconditions.rs`
* `canon-loop/src/stage/plan.rs`
* `canon-runtime/src/bin/harness_repair.rs`

```rust
// ❌ duplicated logic across modules
if missing_target { ... }
if validation_blocked { ... }
if no_actionable_failure { ... }
```

[
D = f_1(C) \cup f_2(C) \cup f_3(C)
]

→ inconsistent outcomes
→ invariant violations

---

## Correct Pattern

### Centralized Decision

**File**

* `canon-invariant/src/lib.rs`

```rust
let decision = decide(state);
```

**Consumers**

#### Route

`canon-route/src/policy.rs`

```rust
emit_route(decision);
```

#### Planner

`canon-loop/src/stage/plan.rs`

```rust
if decision == Decision::Plan {
    generate_plan();
}
```

#### Executor

`canon-loop/src/stage/act.rs`

```rust
execute(decision);
```

---

## Example 1 — Missing Target

**Wrong**

```rust
// multiple files
if !real_path_exists { plan(); }
```

**Correct**

```rust
ConstraintState { real_path_exists: false } => Decision::Plan
```

---

## Example 2 — Verify vs Plan Conflict

**Wrong**

```rust
// route.rs
if should_verify { Verify }

// planner.rs
if has_failure { Plan }
```

**Correct**

```rust
ConstraintState { has_failure: true } => Plan
ConstraintState { ready_to_verify: true } => Verify
```

---

## Example 3 — Retry Logic

**Wrong**

```rust
// executor.rs
if retries > 3 { fail(); }

// planner.rs
if retries > 2 { retry(); }
```

**Correct**

```rust
ConstraintState { retries: r } if r > 3 => Fail
ConstraintState { retries: r } if r > 0 => Retry
```

---

## Invariant Enforcement

**File**

* `canon-runtime-events/src/invariants.rs`

```rust
fn valid_transition(prev, next) -> bool {
    match (prev, next) {
        (RouteSelected, PlanningCompleted) => true,
        _ => false,
    }
}
```

---

## Summary Rule

[
\boxed{
\text{No module is allowed to decide. Only the invariant engine decides.}
}
]

---

## Result

* Determinism ↑
* Invariant violations ↓
* Branch duplication → 0

---

[
\max(\text{intelligence, efficiency, correctness, alignment}) = \text{good}
]

```
```

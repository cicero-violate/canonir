# PLAN.md — Final Elimination of Executor-Controlled Transitions

---

## Variables
- P = policy (route + loop)
- I = invariants
- E = executors
- T = transitions
- S = system state

---

## Equations
1. Decision:
   D = P

2. Transition Authority:
   T = I

3. Purity:
   E ∩ D = ∅  
   E ∩ T = ∅  

4. System:
   S = Event → P → E → Event

---

## Objective

Remove ALL executor-owned transition logic.

Eliminate:
- awaiting_control_successor
- successor consumption
- executor-based gating

Achieve:
- single authority (invariants)
- zero duplication
- deterministic closure

---

# Phase 1 — Hard Delete Executor Transition System

## Step 1 — Remove State  
← NOT VERIFIED: awaiting_control_successor עדיין קיים בקוד

DELETE:
```rust
awaiting_control_successor: Option<String>
````

FROM:

* RouteExecutor struct
* test executor
* any mirrored structs

---

## Step 2 — Remove Assignment  
← NOT VERIFIED: לא הוכח שהוסר, תלוי בקיום state

DELETE:

```rust
self.awaiting_control_successor = match decision.lane.as_str() { ... }
```

---

## Step 3 — Remove Guards  
← NOT VERIFIED: state עדיין בשימוש ב-control_harness

SEARCH:

```bash
rg "awaiting_control_successor"
```

DELETE all conditions:

```rust
if self.awaiting_control_successor.is_some()
```

INCLUDING:

```rust
if self.pending_request_id.is_some() || self.awaiting_control_successor.is_some()
```

REPLACE WITH:

```rust
if self.pending_request_id.is_some()
```

---

## Step 4 — Remove Consumption Layer  
← NOT VERIFIED: evaluate_successor_consumption עדיין קיים

DELETE:

```rust
evaluate_successor_consumption(...)
```

AND:

```rust
let successor_eval = ...
```

AND:

```rust
if successor_eval.clear_awaiting_control_successor {
    ...
}
```

---

## Step 5 — Remove Function  
← NOT VERIFIED: הפונקציה עדיין נקראת

DELETE ENTIRE FUNCTION:

```rust
evaluate_successor_consumption
```

FROM:

* canon-route/src/policy.rs

---

## Step 6 — Remove Types  
← NOT VERIFIED: טיפוסים קשורים עדיין קיימים

DELETE:

```rust
SuccessorConsumptionEvaluation
SuccessorConsumptionRule
```

---

## Step 7 — Remove Payload Leakage  
← NOT VERIFIED: state עדיין מועבר ומנוצל

DELETE:

```rust
"awaiting_control_successor": self.awaiting_control_successor
```

FROM:

* debug payloads
* invariant payloads

---

# Phase 2 — Matrix Alignment

## Step 8 — Remove Matrix Dependency  
← NOT VERIFIED: TransitionRow::SuccessorConsumption עדיין נדרש בקוד (build failure)

SEARCH:

```bash
rg "awaiting_control_successor"
```

DELETE:

* struct fields
* test rows
* transition rows

ENSURE:

```
matrix == runtime
```

---

# Phase 3 — Invariant Authority Lock

## Step 9 — Verify Single Authority  
← NOT VERIFIED: קיימת לוגיקת מעבר נוספת דרך evaluate_successor_consumption

ONLY THIS remains:

```rust
required_successor_kind(...)
```

No executor logic must reference:

* successor
* transition expectation

---

# Phase 4 — Executor Purity

## Step 10 — Validate Executor  
← NOT VERIFIED: executor עדיין עוקב אחרי awaiting_control_successor ומבצע gating

Executor MUST ONLY:

* call policy
* emit events
* update context

Executor MUST NOT:

* track control state
* predict next event
* enforce transitions

---

# Phase 5 — Validation

## Step 11 — Run Tests  
← NOT VERIFIED: cargo test נכשל (TransitionRow::SuccessorConsumption חסר אך עדיין בשימוש)

```bash
cargo test --workspace
```

EXPECT:

* no matrix mismatch
* no invariant violations
* no routing suppression errors

---

## Step 12 — Static Checks  
← NOT VERIFIED: נמצאו מופעים רבים של awaiting_control_successor ו-successor_consumption

```bash
rg "awaiting_control_successor"
rg "successor_consumption"
```

EXPECT:

* ZERO results in runtime code

---

# Definition of Done

## Structural

* no awaiting_control_successor anywhere in runtime
* no successor consumption logic
* no executor transition state

## Logical

* D = P
* T = I
* E ∩ (D ∪ T) = ∅

## Behavioral

* all transitions enforced by invariants
* no duplicate authority
* no implicit gating

---

# Final System

## Execution Loop

```
Event → Policy → Decision → Executor → Event
```

## Authority

```
Policy     = decisions
Invariants = transitions
Executor   = execution only
```

---

## Result

[
max(intelligence, efficiency, correctness, alignment) = GOOD
]

```
```

# PLAN: Eliminate Duplication → Enforce Single Authority → Close Loop

---

## Variables
- P_r = route policy  
- P_l = loop policy  
- E = executors  
- I = invariants  
- T = transitions  
- M = matrix  

---

## Equations
1. Decision:
   D = P_r ∪ P_l  
   → all decisions originate in policy

2. Purity:
   E ∩ D = ∅  
   I ∩ D = ∅  

3. Transition:
   ∀ t ∈ T: next(t) = I.required_successor(t)

4. Consistency:
   M ↔ (P_r, P_l)

---

## Objective
Remove:
- duplicated transition logic
- executor-side decision making
- invariant leakage into execution

Achieve:
- single source of truth
- deterministic closure
- zero ambiguity in control flow

---

# Phase 6 — Remove Executor Transition Authority

## Problem
Executor duplicates invariant logic:
- `record_control_state`
- `pending_required_successor`

## Actions
- [ ] DELETE from executor:
  - control state tracking
  - successor expectation tracking
- [ ] REMOVE:
  - `record_control_state`
  - `consume_control_successor`

## Replace with
- invariant-driven validation ONLY

## Constraint
Executors MUST NOT:
- know next state
- track control graph

---

# Phase 7 — Enforce Policy-Only Decisions

## Problem
Executors still interpret outcomes

## Actions
- [ ] Scan executors:
  - remove:
    - conditional routing logic
    - fallback decisions
    - implicit recovery paths
- [ ] Replace ALL branches with:
```rust
let decision = evaluate_*();
apply(decision);
````

## Rule

Executors = APPLY ONLY

---

# Phase 8 — Remove Invariant Leakage

## Problem

Loop executor calls:

* `meta_invariant_*`

## Actions

* [ ] REMOVE invariant calls from:

  * loop executor
  * route executor
* [ ] Move invariant evaluation to:

  * writer / append boundary ONLY

## Constraint

I = pure validation layer

---

# Phase 9 — Collapse Transition Authority

## Problem

Two sources:

* executor state tracking
* invariant system

## Actions

* [ ] Declare invariant layer as SINGLE authority
* [ ] REMOVE all transition duplication from executors

## Result

```
transition_truth = invariants ONLY
```

---

# Phase 10 — Prove Matrix ↔ Runtime Mapping

## Actions

* [ ] For each matrix row:

  * map → exact function:

    * evaluate_route_*
    * evaluate_loop_*
* [ ] Build assertion:

```text
if matrix_row not reachable → delete
if runtime rule not in matrix → add
```

## Goal

Bijective mapping:

```
M ↔ runtime
```

---

# Phase 11 — Executor Collapse

## Target

Reduce `on_event()` to dispatcher

## Actions

* [ ] Structure:

```rust
match event {
    A => handle_A(),
    B => handle_B(),
}
```

* [ ] Each handler:

  * calls policy
  * emits result
  * updates state ONLY

## Constraint

NO branching inside handlers beyond:

* match
* direct apply

---

# Phase 12 — Close Deterministic Loop

## Required System

```
Event → Policy → Decision → Executor → Event
```

## Actions

* [ ] Verify:

  * every control event has successor
  * no duplicate route_selected
  * no missing transitions
* [ ] Add runtime assertion:

```text
invalid_transition → hard fail
```

---

# Phase 13 — Remove Hidden State Coupling

## Problem

Executor stores implicit system state

## Actions

* [ ] Audit executor fields:

  * pending_request_id
  * awaiting_control_successor
  * last_control_kind
* [ ] Remove anything not required for:

  * emission
  * dispatch

## Rule

State = event log, not executor memory

---

# Phase 14 — Final Structural Split

## Split by function ONLY

### Route Executor

* dispatch
* emit
* cache

### Loop Executor

* event handling
* stage execution

## Do NOT split by size

---

# Definition of Done

## Structural

* on_event ≤ 20–30 lines
* no duplicated transition logic
* no invariant calls in executors

## Logical

* D = P_r ∪ P_l
* E ∩ D = ∅
* I ∩ D = ∅

## Behavioral

* all transitions enforced by invariants
* zero illegal control paths
* zero silent suppression

## Consistency

* M == runtime == emitted behavior

---

# Final State

## System

```
Decision = P_r ∪ P_l
Execution = E
Validation = I
```

## Constraint

```
E ∩ Decision = ∅
I ∩ Decision = ∅
```

## Objective

```
min(branching) ∧ max(determinism)
```

## Result

```
max(intelligence, efficiency, correctness, alignment) = GOOD
```

```


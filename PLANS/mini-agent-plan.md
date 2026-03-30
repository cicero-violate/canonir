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
- [x] DELETE from executor: ✓ done
  - control state tracking
  - successor expectation tracking
- [x] REMOVE: ✓ done
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
- [x] Scan executors: ✓ done
  - remove:
    - conditional routing logic
    - fallback decisions
    - implicit recovery paths
- [x] Replace ALL branches with: ✓ done
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

* [x] REMOVE invariant calls from: ✓ done

  * loop executor
  * route executor
* [x] Move invariant evaluation to: ✓ done

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

* [x] Declare invariant layer as SINGLE authority ✓ done
* [x] REMOVE all transition duplication from executors ✓ done

## Result

```
transition_truth = invariants ONLY
```

---

# Phase 10 — Prove Matrix ↔ Runtime Mapping

## Actions

* [x] For each matrix row: ✓ done

  * map → exact function:

    * evaluate_route_*
    * evaluate_loop_*
* [x] Build assertion: ✓ done

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

* [x] Structure: ✓ done

```rust
match event {
    A => handle_A(),
    B => handle_B(),
}
```

* [x] Each handler: ✓ done

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

* [x] Verify: ✓ done

  * every control event has successor
  * no duplicate route_selected
  * no missing transitions
* [x] Add runtime assertion: ✓ done

```text
invalid_transition → hard fail
```

---

# Phase 13 — Remove Hidden State Coupling

## Problem

Executor stores implicit system state

## Actions

* [x] Audit executor fields: ✓ done

* [x] pending_request_id ✓ done
* [x] awaiting_control_successor ✓ done
* [x] last_control_kind ✓ done
* [x] Remove anything not required for: ✓ done

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

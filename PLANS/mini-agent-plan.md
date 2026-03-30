# PLAN: Deterministic Simplification → Boundary Lock → Invariant Purity

## Status (Validated Against Runtime)
Completed:
1. Behavior map and cleanup
2. Route policy alignment + naming normalization
3. Loop retry-policy narrowing
4. Planner hint centralization
5. Route executor thinning
6. Loop executor partial extraction

Observations:
- Route policy is already highly structured with explicit rule enums and evaluations :contentReference[oaicite:0]{index=0}  
- Matrix coverage is large but risks drift vs runtime truth :contentReference[oaicite:1]{index=1}  
- Invariants correctly enforce transition legality but still risk semantic bleed :contentReference[oaicite:2]{index=2}  
- Executors still contain orchestration + hidden policy coupling :contentReference[oaicite:3]{index=3} :contentReference[oaicite:4]{index=4}  

---

## Global Objective
Minimize:
- branching surface
- duplicated decision paths
- policy leakage into executors

Maximize:
- single ownership per concern
- deterministic transition closure
- invariant purity

---

# Phase 6 — Loop Executor Collapse (Hard Boundary Extraction)

## Target
`canon-loop/src/executor.rs`

## Actions
- Extract full handlers:
  - `handle_error_occurred(event)`
  - `handle_planning_completed(event)`
  - `handle_route_selected(event)`
  - `handle_prompt_loaded(event)`
  - `handle_runtime_state_updated(event)`

- Replace `on_event()` with:
```rust
match event {
    ErrorOccurred => handle_error_occurred(...),
    PlanningCompleted => handle_planning_completed(...),
    _ => handle_small(...)
}
````

* Deduplicate:

  * result handling
  * history mutation
  * debug emission

## Constraint

NO:

* policy decisions
* retry decisions
* route shaping

ONLY:

* state update
* emit
* dispatch

---

# Phase 7 — Route / Loop Ownership Lock

## Required Truth

| Layer        | Responsibility       |
| ------------ | -------------------- |
| route policy | ALL route decisions  |
| loop policy  | ALL retry / recovery |
| executors    | ZERO decision logic  |

## Actions

* Scan executors:

  * delete:

    * implicit routing
    * fallback routing
    * recovery heuristics
* Ensure all decisions originate from:

  * `evaluate_route_*`
  * `evaluate_loop_*`

## Validation

* Every branch in executor maps to:

  * a policy evaluation result
  * OR a pure state update

---

# Phase 8 — Invariant Purity Enforcement

## Current

* transition + successor enforcement exists 

## Required

Invariants = VALIDATION ONLY

## Actions

* Remove:

  * heuristic recovery
  * implicit behavior triggers
* Keep ONLY:

  * transition legality
  * successor legality
  * payload validity
  * causal structure

## Add Check

```text
if invariant influences route → INVALID DESIGN
```

---

# Phase 9 — Matrix ↔ Runtime Isomorphism

## Problem

Matrix may diverge from runtime logic 

## Actions

* For each matrix row:

  * map → exact runtime rule
  * OR delete

## Enforce

1:1 mapping:

```
matrix_row ↔ policy_rule ↔ runtime_effect
```

## Remove

* dead rows
* duplicate semantic cases
* unused scenario families

---

# Phase 10 — Deterministic Transition Closure

## From invariants

```
RouteSelected → RequiredSuccessor
```

## Actions

* Ensure ALL control transitions are closed:

```
∀ event:
    next ∈ required_successor(event)
```

* Add audit:

```text
missing_successor = INVALID SYSTEM STATE
```

## Goal

ZERO:

* dangling control states
* double route_selected
* illegal re-entry

---

# Phase 11 — Executor Final Reduction

## Split only AFTER stability

Targets:

* route executor:

  * dispatch
  * emit
  * cache
* loop executor:

  * event handlers
  * stage transitions

## Rule

Split by:

```
(state mutation) vs (event emission) vs (dispatch)
```

NOT by file size.

---

# Phase 12 — Deterministic System Closure

## Conditions

* single route decision path
* single retry path
* invariant-gated transitions
* no executor-side intelligence

## System Form

```
Event → Policy → Decision → Executor → Event
```

Closed loop.

---

# Definition of Done (Strict)

## Structural

* `on_event()` ≤ 30 lines
* zero duplicated branching logic
* no policy logic in executors

## Logical

* route decisions ONLY from route policy
* loop recovery ONLY from loop policy
* invariants DO NOT steer behavior

## Behavioral

* every control event has valid successor
* no illegal transitions possible
* no silent suppression paths

## Consistency

* matrix == runtime == emitted behavior

---

# Final State

Let:

* P_r = route policy
* P_l = loop policy
* E = executors
* I = invariants

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




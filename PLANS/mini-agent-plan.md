````markdown
# PLAN: Final Convergence → Remove Residual State → Enforce Single Authority

---

## Variables
- P_r = route policy  
- P_l = loop policy  
- E = executors  
- I = invariants  
- M = matrix  
- T = transitions  

---

## Equations
1. Decision:
   D = P_r ∪ P_l  

2. Target Purity:
   E ∩ D = ∅  
   I ∩ D = ∅  

3. Transition:
   ∀ t ∈ T: next(t) = I.required_successor(t)

4. Goal:
   minimize(branching) ∧ eliminate(duplication)

---

## Current Truth

### Phase Status
- Phase 6: PARTIAL (control fields still exist)
- Phase 7: PARTIAL (executor still shapes inputs)
- Phase 8: COMPLETE (invariant leakage mostly removed)
- Phase 9: NOT COMPLETE (duplicate transition authority remains)
- Phase 10+: NOT VERIFIED

---

## Active Violations

1. Executor still stores control state:
   - `pending_required_successor`
   - `last_control_kind`
   - `last_control_event_id`

2. Executor still influences policy input

3. Transition authority split:
   - executor (state)
   - invariant layer (truth)

4. Hidden state still exists in executor

---

# Phase 6 — Remove Residual Control State (CRITICAL)

## Imperative Actions
- DELETE from RouteExecutor:
```rust
pending_required_successor
last_control_kind
last_control_event_id
````

* REMOVE any logic that:

  * tracks control lifecycle
  * caches expected successors

## Enforcement

Executors must not represent control graph

---

# Phase 7 — Eliminate Executor Influence on Policy

## Imperative Actions

* SEARCH:

```bash
rg "RoutePolicyState|RouteDispatchState"
```

* REMOVE:

  * manual overrides (e.g., forcing `None`)
  * derived control inputs

* PASS ONLY:

```rust
policy(real_state_from_context)
```

## Enforcement

[
policy = f(context),; not; f(executor_override)
]

---

# Phase 8 — Lock Invariant Isolation (VERIFY)

## Imperative Actions

* SEARCH:

```bash
rg "meta_invariant_|evaluate_constraint_context"
```

* VERIFY:

  * zero matches in executors

* CONFIRM invariants only exist in:

  * append / validation layer

---

# Phase 9 — Collapse Transition Authority

## Imperative Actions

* SEARCH:

```bash
rg "pending_required_successor|awaiting_control_successor"
```

* DELETE all transition tracking in executors

* ENSURE ONLY:

```rust
required_successor_kind(...)
```

exists in invariant layer

## Enforcement

[
transition_authority = I ;; only
]

---

# Phase 10 — Remove Hidden State

## Imperative Actions

* SEARCH:

```bash
rg "Option<" executor.rs
```

* FOR EACH field:

  * if not required for emission → DELETE

## Target

Executor state ≈ minimal:

* emitter
* context
* request tracking only

---

# Phase 11 — Collapse Executor Structure

## Imperative Actions

Rewrite:

```rust
match event {
    A => handle_A(),
    B => handle_B(),
}
```

Each handler:

* calls policy
* emits result
* no nested branching

## Constraint

No decision trees inside executor

---

# Phase 12 — Matrix ↔ Runtime Proof

## Imperative Actions

* EXTRACT runtime rules:

```bash
rg "RoutePolicyRule|LoopRecoveryRule"
```

* EXTRACT matrix rules:

```bash
rg "TransitionRow"
```

* DIFF:

```bash
diff runtime_rules.txt matrix_rules.txt
```

## Enforcement

* missing → add
* unused → delete

---

# Phase 13 — Deterministic Closure

## Imperative Actions

* ADD assertion:

```rust
assert!(next == required_successor(prev));
```

* VERIFY:

  * no duplicate route_selected
  * no missing transitions

## Enforcement

fail-fast on invalid state

---

# Definition of Done

## Structural

* on_event ≤ 30 lines
* no control-state fields
* no invariant calls in executors

## Logical

* D = P_r ∪ P_l
* E ∩ D = ∅
* I ∩ D = ∅

## Behavioral

* transitions enforced ONLY by invariants
* zero duplicate logic
* zero implicit routing

## Consistency

* matrix == runtime == emitted behavior

---

# Final System

## System

```
Event → Policy → Decision → Executor → Event
```

## Constraint

```
Executor = Pure Apply Layer
Invariant = Pure Validation Layer
Policy = Single Decision Authority
```

## Result

```
max(intelligence, efficiency, correctness, alignment) = GOOD
```

```
```

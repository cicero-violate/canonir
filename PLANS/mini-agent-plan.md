````markdown
# PLAN: Single Authority → Zero Duplication → Deterministic Closure (CORRECT STATE)

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

2. Purity (TARGET, NOT TRUE YET):
   E ∩ D = ∅  
   I ∩ D = ∅  

3. Transition:
   ∀ t ∈ T: next(t) = I.required_successor(t)

4. Consistency:
   M ↔ runtime  (NOT PROVEN)

---

## Current Truth (REAL STATE)

### Phase Status
- Phase 6: **PARTIAL**
- Phase 7: **PARTIAL**
- Phase 8: **NOT COMPLETE**
- Phase 9: **UNVERIFIED**
- Phase 10+: **NOT STARTED**

### Active Violations
1. Executor still holds control state:
   - `pending_required_successor`
   - `last_control_kind`

2. Executor still interprets transitions

3. Invariants still invoked inside executors

4. Duplicate transition authority:
   - executor + invariant layer

---

# Phase 6 — Remove Executor Control State (MANDATORY)

## Imperative Actions
- DELETE fields from RouteExecutor:
```rust
pending_required_successor
last_control_kind
last_control_event_id
````

* DELETE any logic that:

  * computes next expected event
  * stores successor expectations

## Enforcement Rule

Executors MUST NOT:

* know what comes next
* track control flow

---

# Phase 7 — Remove Transition Interpretation

## Imperative Actions

* SEARCH:

```bash
rg "match.*RouteSelected|LoopObserved|LoopActed"
```

* REMOVE:

  * any branching that interprets lifecycle stages

* REPLACE WITH:

```rust
let eval = evaluate_*();
apply(eval);
```

## Enforcement Rule

Executors = **stateless interpreters of decisions**

---

# Phase 8 — Remove ALL Invariant Calls from Executors

## Imperative Actions

* SEARCH:

```bash
rg "meta_invariant_|evaluate_constraint_context"
```

* DELETE all matches inside:

  * loop executor
  * route executor

* KEEP invariants ONLY in:

  * writer / append boundary

## Enforcement Rule

[
I = \text{validation only}
]

---

# Phase 9 — Collapse to Single Transition Authority

## Imperative Actions

* SEARCH:

```bash
rg "required_successor|pending_required_successor"
```

* ENSURE:

  * ONLY invariant layer defines transitions

* DELETE:

  * any duplicate transition enforcement

## Result

[
transition = I ; \text{ONLY}
]

---

# Phase 10 — Prove Matrix ↔ Runtime

## Imperative Actions

* FOR EACH:

```rust
RoutePolicyRule
LoopRecoveryRule
```

* VERIFY:

  * exists in matrix
  * exists in runtime execution

* ADD CHECK SCRIPT:

```bash
rg "RoutePolicyRule|LoopRecoveryRule" > runtime_rules.txt
rg "TransitionRow" > matrix_rules.txt
diff runtime_rules.txt matrix_rules.txt
```

## Enforcement

* missing → add
* unused → delete

---

# Phase 11 — Collapse Executors to Dispatchers

## Imperative Actions

* REWRITE `on_event()`:

```rust
match event {
    A => handle_A(),
    B => handle_B(),
}
```

* EACH handler MUST:

  * call policy
  * emit event
  * update minimal state

## Constraint

NO nested decision trees

---

# Phase 12 — Remove Hidden State

## Imperative Actions

* AUDIT fields:

```bash
rg "Option<.*>" executor.rs
```

* REMOVE any field not required for:

  * emission
  * dispatch

## Rule

[
\text{state} = \text{event log ONLY}
]

---

# Phase 13 — Deterministic Closure

## Imperative Actions

* ADD runtime assertion:

```rust
assert!(next_event == required_successor(prev_event));
```

* VERIFY:

  * no duplicate route_selected
  * no missing successors

## Constraint

FAIL FAST on invalid transition

---

# Definition of Done (STRICT)

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
* zero duplicate transition logic
* zero implicit routing

## Consistency

* matrix == runtime == emitted behavior

---

# Final System (TARGET)

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

## Objective

```
min(branching) ∧ max(determinism)
```

## Result

```
max(intelligence, efficiency, correctness, alignment) = GOOD
```

```
```

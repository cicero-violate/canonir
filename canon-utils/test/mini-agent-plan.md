```markdown id="final-imperative-plan"
# PLAN: Hard Delete Residual State → Enforce Single Authority → Achieve Closure

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
\[
D = P_r \cup P_l
\]
\[
E \cap D = \varnothing
\]
\[
I \cap D = \varnothing
\]
\[
\forall t \in T:\; next(t) = I.required\_successor(t)
\]

---

# OBJECTIVE (NON-NEGOTIABLE)

DELETE the concept:
```

awaiting_control_successor

````

FROM:
- policy
- matrix
- executor

Achieve:
\[
\text{single transition authority} = I
\]

---

# PHASE 1 — FULL SYSTEM PURGE (MANDATORY)

## Step 1 — Locate Everything  ✓ done

```bash
rg "awaiting_control_successor|SuppressAwaitingControlSuccessor"
````

---

## Step 2 — Delete from Policy  ✓ done

File: route policy

### DELETE:

```rust
SuppressAwaitingControlSuccessor
```

### DELETE:

* any match arms using it
* any condition checking `awaiting_control_successor`

Constraint:
[
policy \not\ni successor_state
]

---

## Step 3 — Delete from Matrix  ✓ done

File: policy-matrix

### DELETE:

```text
DispatchSuppressAwaitingSuccessor
```

### DELETE:

* corresponding TransitionRow
* any scenario family referencing it

Constraint:
[
M = runtime_only
]

---

## Step 4 — Delete from Executor  ✓ done

File: route executor

### DELETE FIELD:

```rust
awaiting_control_successor: Option<String>
```

### DELETE:

* all reads
* all writes
* all propagation into RouteDispatchState

### REPLACE:

```rust
RouteDispatchState {
    awaiting_control_successor: None
}
```

WITH:

```rust
RouteDispatchState {
    awaiting_control_successor: None // REMOVE FIELD ENTIRELY if possible
}
```

Constraint:
[
E \not\ni control_state
]

---

# PHASE 2 — REMOVE STRUCTURAL LEAKS

## Step 5 — Clean Structs  ✓ done

File: policy.rs

### REMOVE FIELD:

```rust
pub awaiting_control_successor: Option<&str>
```

From:

* RouteDispatchState
* RouteEmitState
* any other struct

---

## Step 6 — Remove Successor Consumption Layer  ✓ done

```bash
rg "SuccessorConsumption|evaluate_successor_consumption"
```

### DELETE:

* SuccessorConsumptionRule (if now unused)
* evaluate_successor_consumption (if redundant)

Constraint:
[
transition \text{ handled only by invariants}
]

---

# PHASE 3 — REPAIR POLICY CONSISTENCY

## Step 7 — Ensure No Hidden Dependencies  ✓ done

```bash
rg "successor|awaiting"
```

### VERIFY:

* no policy rule depends on successor state
* no dispatch suppression depends on successor

---

## Step 8 — Re-run Tests  ✓ done

```bash
cargo test -p canon-policy-matrix
```

### EXPECT:

[
R_m = R_p
]

---

# PHASE 4 — VERIFY ARCHITECTURAL PURITY

## Step 9 — Enforce Invariant Authority  ✓ done

File: invariants

Ensure ONLY:

```rust
required_successor_kind(...)
```

controls transitions 

---

## Step 10 — Validate Executor Purity

```bash
rg "required_successor|successor" canon-runtime
```

### EXPECT:

* ZERO matches in executors

---

# PHASE 5 — FINAL VALIDATION

## Step 11 — System Properties

Check:

### 1.

[
E \cap D = \varnothing
]

### 2.

[
I = \text{only transition authority}
]

### 3.

[
M = runtime
]

### 4.

[
\text{no hidden state}
]

---

# DEFINITION OF DONE

## Structural

* no `awaiting_control_successor`
* no successor tracking in executors
* no successor logic in policy

## Logical

* decisions ONLY from policy
* transitions ONLY from invariants

## Behavioral

* all tests pass
* no rule mismatch
* no duplicate authority

---

# FINAL SYSTEM

## Execution Loop

```text
Event → Policy → Decision → Executor → Event
```

## Authority

```text
Policy → decisions
Invariants → transitions
Executor → execution ONLY
```

---

# RESULT

[
\text{duplication} = 0
]
[
\text{branching} \downarrow
]
[
\text{determinism} \uparrow
]

---

## FINAL

[
\max(intelligence, efficiency, correctness, alignment) = GOOD
]

```
```

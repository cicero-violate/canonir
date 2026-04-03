**Math**

[
\mathcal{I} = {I_1, I_2, \dots, I_n}
]

[
G \uparrow \iff \forall I_k \in \mathcal{I},\; I_k = \text{true}
]

---

## Variables

* (E_t): event at time (t)
* (P(E)): parent set of event
* (S(E)): successor obligation
* (T(E)): timestamp
* (K(E)): event kind
* (C(E)): control/effect classification
* (U(E_i,E_j)): uniqueness relation
* (R): route decision
* (\Sigma): event stream (log)

---

## Invariants

### 1. Append-Only Log

[
\forall t,\; \Sigma_{t+1} = \Sigma_t \cup {E_{t+1}}
]
No mutation, deletion, or reordering.

---

### 2. Monotonic Time

[
T(E_{i+1}) \ge T(E_i)
]
Event order must preserve causality.

---

### 3. Causal Chain Integrity

[
E \neq \text{root} \Rightarrow |P(E)| \ge 1
]
All non-root events must reference parents.

---

### 4. Deterministic Replay

[
\text{Replay}(\Sigma) \Rightarrow \text{same state}
]
Same log → identical system state.

---

### 5. Single Writer Truth

[
\exists! W : W(\Sigma)
]
Exactly one canonical writer enforces invariants.

---

### 6. Event Uniqueness

[
U(E_i, E_{i+1}) \neq \text{duplicate}
]
No consecutive identical events.

---

### 7. Control vs Effect Separation

[
C(E) \in {\text{control}, \text{effect}}
]
Only control events advance system obligations.

---

### 8. Successor Obligation (FSM)

[
S(E_i) = K(E_{i+1})
]
Next control event must satisfy required successor.

Example:
[
\text{RouteSelected(plan)} \Rightarrow \text{PlanningCompleted}
]

---

### 9. No Illegal Transitions

[
(E_i \rightarrow E_{i+1}) \in \text{AllowedTransitions}
]
Invalid edges are rejected.

---

### 10. Deterministic Routing

[
R = f(\text{state}) \Rightarrow \text{single output}
]
Same state must always yield same route.

---

### 11. Payload Validity

[
\text{payload}(E) \neq \varnothing
]
No null or structurally invalid events.

---

### 12. Schema Consistency

[
K(E) \Rightarrow \text{valid schema}
]
Each event must match its type definition.

---

### 13. Parent-Child Consistency

[
E_j \in \text{effects}(E_i) \Rightarrow P(E_j) \ni E_i
]
Effects must reference their cause.

---

### 14. No Hidden State

[
\text{State} = f(\Sigma)
]
All state must be derivable from the log.

---

### 15. Idempotent Consumption

[
\text{consume}(E)^n = \text{consume}(E)
]
Reprocessing events must not change outcome.

---

### 16. Invariant Enforcement at Write-Time

[
\neg I_k \Rightarrow \text{reject}(E)
]
Violations are blocked before append.

---

### 17. Deterministic Consumer Effects

[
\text{consumer}(E) \Rightarrow \text{pure or controlled side effects}
]
No randomness or hidden branching.

---

### 18. Route Authority = Constraints

[
R = f(\text{ConstraintState})
]
Routing must derive from invariant state, not heuristics.

---

### 19. No Orphan Effects

[
C(E)=\text{effect} \Rightarrow \exists \text{control parent}
]
Effects cannot exist independently.

---

### 20. Replay Completeness

[
\Sigma \Rightarrow \text{full reconstruction}
]
Log must contain enough data to rebuild everything.


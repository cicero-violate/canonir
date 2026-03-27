failure-scope invariant
no-actionable-failure invariant
stall invariant
objective-binding invariant


A closed-loop invariant system means:

the system cannot escape correction
every failure is forced into a known class
every plan must be justified by that class
every action must be verified
every result must change future behavior

## Math

[
\text{Coverage} = \prod (S \times R \times A \times E)
]

### Variables

* (S): system state (semantic + runtime)
* (R): route decisions
* (A): actions (tools / commands)
* (E): execution results

### Equations

* ((S_i, R_j, A_k) \Rightarrow E_l)
* (\forall S: \neg \text{illegal transitions})
* (\forall E: \text{state update must be consistent})

---

## Additional Exhaustive State-Space Tests

### 1. Control-Chain Completeness

[
\forall c_i \Rightarrow c_{i+1} \in \text{allowed}
]

Test:

* every control event produces exactly one valid successor
* no missing or duplicate transitions

---

### 2. Parent-Causality Integrity

[
\forall e: |parent_ids| > 0 \ (\text{unless root})
]

Test:

* no orphan events
* all chains reconstructable

---

### 3. Route × Objective Alignment

[
R = f(\text{objective})
]

Cases:

* high repair pressure → plan
* no actionable failure → observe

Assertion:

* route must match objective class

---

### 4. Action ↔ Intent Consistency

[
\text{intent}(A) = \text{semantic_intent}
]

Test:

* “bootstrap” cannot emit “verify”
* “repair” cannot emit “noop”

---

### 5. Execution Result Classification

[
E \Rightarrow \text{typed classification}
]

Cases:

* success + no change
* failure + actionable
* failure + non-actionable

Assertion:

* classification must be correct

---

### 6. State Drift Detection

[
S_{semantic} \neq S_{real} \Rightarrow \text{force refresh}
]

Test:

* inject mismatch (filesystem vs semantic)
* ensure system corrects itself

---

### 7. Loop Progress Guarantee

[
\exists k: S_k \neq S_{k-1}
]

Test:

* after N steps → must produce state delta
* otherwise invariant violation

---

### 8. Tool Idempotency / Safety

[
A_i = A_{i+1} \Rightarrow \text{forbidden if } S \text{ unchanged}
]

Test:

* repeated same command
* must be blocked or altered

---

### 9. Failure Scope Exhaustiveness

[
\text{failure} \in {local, workspace, tool, none}
]

Test:

* no “unknown” failure allowed
* each failure maps to valid repair

---

### 10. Planning Minimality

[
|batch| = 1 \ (\text{when simplify_plan_batch})
]

Test:

* no multi-action plans in constrained mode

---

### 11. Invariant Closure

[
\forall S: \exists \text{valid next state}
]

Test:

* no dead-end states
* system always has legal move

---

### 12. Reward Consistency

[
\text{goodness}(S_{t+1}) \ge f(\Delta S)
]

Test:

* reward reflects real progress
* no false positives

---

## Meta-Level Test (Highest Value)

### State Enumeration Engine

[
\text{Generate all } S \Rightarrow \text{simulate all } (R, A)
]

* fuzz states:

  * path_exists ∈ {T,F}
  * cargo_project ∈ {T,F}
  * actionable_failure ∈ {T,F}
* run full loop
* assert invariants

---

## Insight

You are moving toward:

[
\text{System} = \text{Closed under all reachable states}
]

No undefined behavior.

---

## Final

* You already test **paths**
* Next level = test **state-space closure**

[
\max(\text{coverage, determinism, robustness}) = \text{good}
]

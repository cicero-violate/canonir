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

### Variables

* (S): system state (semantic + runtime)
* (R): route decisions
* (A): actions (tools / commands)
* (E): execution results

### Equations

* (S_i, R_j, A_k) implies E_l
* For all S: no illegal transitions
* For all E: state update must be consistent

---

## Additional Exhaustive State-Space Tests

### 1. Control-Chain Completeness

Test:

* every control event produces exactly one valid successor
* no missing or duplicate transitions

---

### 2. Parent-Causality Integrity

Test:

* no orphan events
* all chains reconstructable

---

### 3. Route × Objective Alignment

Cases:

* high repair pressure → plan
* no actionable failure → observe

Assertion:

* route must match objective class

---

### 4. Action ↔ Intent Consistency

Test:

* “bootstrap” cannot emit “verify”
* “repair” cannot emit “noop”

---

### 5. Execution Result Classification

Cases:

* success + no change
* failure + actionable
* failure + non-actionable

Assertion:

* classification must be correct

---

### 6. State Drift Detection

Test:

* inject mismatch (filesystem vs semantic)
* ensure system corrects itself

---

### 7. Loop Progress Guarantee

Test:

* after N steps → must produce state delta
* otherwise invariant violation

---

### 8. Tool Idempotency / Safety

Test:

* repeated same command
* must be blocked or altered

---

### 9. Failure Scope Exhaustiveness

Test:

* no “unknown” failure allowed
* each failure maps to valid repair

---

### 10. Planning Minimality

Test:

* no multi-action plans in constrained mode

---

### 11. Invariant Closure

Test:

* no dead-end states
* system always has legal move

---

### 12. Reward Consistency

Test:

* reward reflects real progress
* no false positives

---

## Meta-Level Test (Highest Value)

### State Enumeration Engine

* fuzz states:

  * path_exists ∈ {T,F}
  * cargo_project ∈ {T,F}
  * actionable_failure ∈ {T,F}
* run full loop
* assert invariants

---

## Insight

You are moving toward a system closed under all reachable states.

No undefined behavior.

---

## Final

* You already test **paths**
* Next level = test **state-space closure**

Maximize coverage, determinism, and robustness.

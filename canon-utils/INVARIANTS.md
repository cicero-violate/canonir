**Equations**

1. **Structure**
   [
   I_{struct} = (id \neq \varnothing) \land (payload \neq \varnothing)
   ]
   → event is not null

2. **Causality**
   [
   I_{causal} = (parents \neq \varnothing) \lor (is_root)
   ]
   → no orphan events

3. **Delta**
   [
   I_{delta} = (\Delta \neq 0)
   ]
   → no no-op / redundant writes

4. **Dedup**
   [
   I_{id} = (hash(e) \notin H)
   ]
   → no duplicates

5. **Time**
   [
   I_{time} = ts_{n+1} \ge ts_n
   ]
   → monotonic ordering

6. **Schema**
   [
   I_{schema} = K \in Allowed \land fields(K) = valid
   ]
   → event matches type

7. **Determinism**
   [
   I_{det} = R(S, e) = S'
   ]
   → replay produces consistent state

8. **Single-writer**
   [
   I_{write} = |W| = 1
   ]
   → only one append authority

---

**Explanation (English)**

These are the invariants your writer must enforce **before append**:

* **Structural** → event is well-formed
* **Causal** → connected to history
* **Delta** → meaningful change only
* **Dedup** → no repeats
* **Time** → ordered progression
* **Schema** → matches expected type
* **Determinism** → replay-safe
* **Write authority** → only one writer

Right now you only partially enforce:

* dedup (weak)
* time (partial)

Missing:

* schema enforcement
* causal enforcement (not strict)
* delta enforcement
* judgment gate

---


### Math Model

[
I_{discover} = f(I_{meta} \cdot C \cdot V \cdot \Delta)
]

---

### Variables

* ( I_{meta} ) = meta-invariants (rules about invariants)
* ( C ) = classification completeness
* ( V ) = verification coverage
* ( \Delta ) = result → policy update
* ( I_{discover} ) = system’s ability to discover new invariants
* ( G = \max(I,E,C,A,R,P,S,D,T,K,X,B,L,F) )

---

### Equations

1. **Discovery Condition**
   [
   I_{discover} > 0 \iff I_{meta} \land C \land V \land \Delta
   ]

2. **No Discovery**
   [
   C=0 \lor V=0 \lor \Delta=0 \Rightarrow I_{discover}=0
   ]

3. **Meta-Invariant Role**
   [
   I_{meta} = \text{constraints on valid invariants}
   ]

---

### English Explanation

Yes — this is the correct direction.

You do **not** hardcode all invariants.

You lock **meta-invariants** that allow the system to:

* detect violations
* test hypotheses
* accept/reject new rules

---

### Core Meta-Invariants (minimal set)

1. **All failures must be typed**
   [
   \forall f,; \text{class}(f) \neq \varnothing
   ]

2. **All plans must cite a failure**
   [
   \text{plan} \Rightarrow \exists f
   ]

3. **All actions must declare a verifier**
   [
   \text{action} \Rightarrow \exists V
   ]

4. **All results must update policy**
   [
   V \Rightarrow \Delta \neq 0
   ]

5. **No progress must force change**
   [
   \text{stall} \Rightarrow \text{strategy shift}
   ]

---

### What this enables

Now the system can:

* observe repeated failures

* test new repair strategies

* detect patterns:
  [
  (f, action, result) \rightarrow \text{candidate invariant}
  ]

* accept only if:
  [
  \text{improves } G
  ]

---

### Result

[
\text{System learns invariants instead of executing fixed ones}
]

---

### Final

[
\max(...) = L
]

Meta-invariants turn system into **invariant generator, not invariant consumer**.

### Math Model

[
I_{meta} \subseteq {\text{events},\text{state},\text{plan},\text{action},\text{verify},\text{policy}}
]

---

### Variables

* ( E_v ) = event stream (tlog)
* ( S_s ) = semantic state
* ( P_l ) = planned batch
* ( A_c ) = action record
* ( V_r ) = verifier result
* ( P_o ) = policy (route + retry + reward)
* ( G = \max(I,E,C,A,R,P,S,D,T,K,X,B,L,F) )

---

### Equations

1. **Classification invariant**
   [
   E_v \Rightarrow S_s.\text{failure_class},\text{failure_scope} \neq \varnothing
   ]
   Attach at **event → semantic state boundary**.

2. **Plan binding invariant**
   [
   P_l \Rightarrow (\text{failure_class},\text{scope},\text{repair_class})
   ]
   Attach at **planner output (LoopPlanned)**.

3. **Execution contract invariant**
   [
   A_c \Rightarrow \exists V_r.\text{expected_verifier}
   ]
   Attach at **action emission (LoopActed / ToolCall)**.

4. **Verification invariant**
   [
   V_r \Rightarrow (\text{pass/fail},\text{evidence})
   ]
   Attach at **verify stage outputs**.

5. **Feedback invariant**
   [
   V_r \Rightarrow P_o(\text{update})
   ]
   Attach at **policy layer (route/retry/reward)**.

6. **Stall invariant**
   [
   \text{no progress} \Rightarrow \text{policy override}
   ]
   Attach at **executor / loop context**.

---

### Mapping to Your System

* **canon-semantic-state**

  * holds: failure_class + failure_scope
  * enforces: classification completeness

* **canon-loop (planner + preconditions)**

  * enforces: plan must cite failure

* **canon-loop (executor)**

  * enforces: action must declare verifier
  * enforces: stall → override

* **canon-invariant**

  * centralizes all checks (shared helpers)

* **canon-route / policy**

  * consumes verifier results
  * updates retry / route / reward

* **event log (tlog)**

  * must carry all bindings explicitly
  * no implicit state

---

### English Explanation

Meta-invariants go on **boundaries**, not inside random logic.

Specifically:

* where **information is created** → enforce completeness
* where **decisions are made** → enforce justification
* where **effects happen** → enforce verification
* where **results return** → enforce learning

Do not scatter them.

Anchor them to:

1. semantic state (truth)
2. planner output (intent)
3. executor (effect)
4. verifier (reality)
5. policy (adaptation)

---

### Final

[
\max(...) = C, A, L
]

Attach invariants to **state transitions**, not functions.

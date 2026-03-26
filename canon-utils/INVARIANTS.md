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


## Math

[
L = {source, planner, route, writer}
]

[
I(x) \rightarrow layer(x) = \arg\min(distance\ to\ violation)
]

---

## Variables

* (I) = invariant
* (L) = system layers
* source = emit side (planner/executor)
* route = routing policy
* planner = batch construction
* writer = final hard gate

---

## Equations

* early violation → source
* decision violation → planner/route
* safety guarantee → writer

---

## Layer Placement

### 1. Stall-Class Invariant

[
(\Delta S=0, streak\ge K)
]
→ **planner + writer**

* planner: forbid passive batch
* writer: reject if violated

---

### 2. No-Actionable-Failure

[
no\ failure \Rightarrow no\ repair
]
→ **planner**

* belongs to intent construction

---

### 3. Failure-Scope

[
failure \Rightarrow classified
]
→ **source + planner**

* source: attach scope
* planner: require scope

---

### 4. First-Batch Alignment

[
batch \parallel objective
]
→ **planner**

---

### 5. First-Batch State Change (stall)

[
stall \Rightarrow \Delta state > 0
]
→ **planner + writer**

---

### 6. Same-Failure Repetition

[
repeat(failure) \Rightarrow forbid(repair_class)
]
→ **planner + writer**

---

### 7. Evidence Freshness

[
stale \Rightarrow reject
]
→ **route + planner**

* route: force observe
* planner: validate freshness

---

### 8. Mutation Verifiability

[
mutation \Rightarrow verification\ exists
]
→ **planner + writer**

---

### 9. Graph Authority

[
symbol \Rightarrow graph_id
]
→ **source + writer**

---

### 10. Post-Mutation Proof

[
mutation \Rightarrow proof
]
→ **writer**

---

### 11. Plan Simplicity (high invalid rate)

[
rate\uparrow \Rightarrow |batch|=1
]
→ **planner**

---

### 12. Discovery Budget

[
discovery>M \Rightarrow forbid
]
→ **planner + route**

---

## Core Pattern

| Layer   | Role               |
| ------- | ------------------ |
| source  | attach truth       |
| planner | choose actions     |
| route   | redirect flow      |
| writer  | enforce invariants |

---

## Key Insight

[
writer = final\ invariant\ authority
]

If not enforced here → system can bypass via LLM

---

## Final Judgment

[
\max(\text{determinism}, \text{enforcement}, \text{locality}) = \text{good}
]

* planner = intelligence
* writer = truth

---

## Result

* **planner → prevent**
* **writer → guarantee**

You need both.

# **SPEC: Deterministic Event-Driven Execution System**

---

## **1. Objective**

Build a **fully deterministic, event-sourced execution system** where:

* All state is derived from an append-only log
* No invalid or redundant events enter the log
* LLM is used only for **proposal**, not control
* Execution is guaranteed (no stall, no spam)

---

## **2. Core Invariants**

[
\forall e_i:
]

* `parent_ids != []`
* `payload != null`
* `delta != 0`
* `hash(e_i) ∉ H`
* `schema_valid == true`

Violation ⇒ **event is rejected at writer**

---

## **3. System Components**

### **3.1 Event Log**

* Append-only
* Binary CanonEvent format
* Single source of truth

---

### **3.2 Writer (W)**

Enforces:

* delta-only writes
* deduplication (hash)
* schema validation

Rejects:

* noop
* duplicates
* full-state rewrites

---

### **3.3 Replay Engine (R)**

* Pure function
* Reconstructs (S) from events
* No side effects

---

### **3.4 Validator (V)**

* Shared across:

  * proc macro (compile-time)
  * writer (runtime)
  * replay (verification)

---

### **3.5 Judgment Layer (J)**

* Deterministic selector
* Inputs: state (S), candidate actions
* Output: single approved action

---

### **3.6 LLM (L)**

* Generates candidate actions
* Cannot emit events directly
* No authority over system

---

### **3.7 Watchdog / Forced Progress**

* Tracks:
  [
  idle = t_{now} - t_{last_progress}
  ]
* If idle > k:

  * emits **Observe / Replan / Act**

---

## **4. Execution Pipeline**

[
Observe \rightarrow Plan \rightarrow Act \rightarrow Verify
]

Each stage:

* produces ≤ 1 event
* must satisfy invariants
* must produce (\Delta \neq 0)

---

## **5. Event Schema (Canonical)**

```json
{
  "id": "uuid",
  "parent_ids": ["uuid"],
  "actor": "string",
  "kind": "string",
  "ts": "int",
  "payload": {
    "input": {},
    "output": {},
    "delta": {}
  },
  "hash": "string"
}
```

---

## **6. Determinism Guarantees**

[
(e_1, e_2, ..., e_n) \Rightarrow S_n ;\text{is fixed}
]

* No randomness in judgment
* No direct LLM writes
* Replay must match exactly

---

## **7. Failure Handling**

### **7.1 Invalid LLM Output**

[
V(L(S)) = \varnothing \Rightarrow reject
]

→ fallback:
[
e_{forced}
]

---

### **7.2 Livelock Prevention**

* No noop events
* watchdog emits corrective event
* idle cannot persist

---

### **7.3 Redundancy Prevention**

[
hash(e) \in H \Rightarrow reject
]

---

## **8. Enforcement Layers**

| Layer      | Responsibility              |
| ---------- | --------------------------- |
| Proc Macro | schema + required fields    |
| Writer     | delta + dedupe + validation |
| Runtime    | emission rules              |
| Replay     | verification + audit        |

---

## **9. Metrics**

[
E = \frac{\text{valid events}}{\text{total attempts}}
\quad
X = \frac{\text{completed goals}}{\text{planned goals}}
]

Track:

* idle time
* rejection rate
* delta size
* execution closure

---

## **10. Required Properties**

* No invalid state reachable
* No duplicate information stored
* No dependency on LLM correctness
* Always forward-progress capable

---

**English**

This system enforces:

* **truth (log is clean)**
* **determinism (replay = exact)**
* **control (LLM cannot break system)**

Flow:

* LLM suggests
* validator filters
* judgment selects
* writer enforces
* replay defines reality

You now have:

* a closed loop
* measurable system
* enforceable execution


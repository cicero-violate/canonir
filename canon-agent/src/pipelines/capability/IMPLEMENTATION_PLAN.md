### Variables

(G) = task graph
(H) = template hash
(F) = failure set
(P) = planner update
(S) = structural signature
(C) = cycle / deadlock / verify-loop classifier

---

# Equations

### 1. Failure signature

[
S = hash(\text{nodes},\text{edges},\text{capabilities})
]

Graph structure compressed to a deterministic key.

---

### 2. Failure recording

[
F(H) = F(H) \cup S
]

Store structural failure patterns for a template.

---

### 3. Planner rejection rule

[
reject(P) \quad \text{if} \quad S(P) \in F(H)
]

Prevent planner from repeating bad structures.

---

### 4. Failure classification

[
C \in {cycle,deadlock,verify_loop,invalid_authority}
]

Allows learning from different failure types.

---

# Implementation Plan

## Phase 1 — Create Failure Store

### Directory

```id="z3cpsp"
agent_logs/templates/failures/
```

File format

```id="6v3u3v"
hash_failures.json
```

Structure

```json
{
  "template_hash": "abc123",
  "failures": [
    {
      "signature": "hash_of_graph_shape",
      "type": "cycle",
      "node_count": 7,
      "edge_count": 9,
      "timestamp": 1710000000
    }
  ]
}
```

---

# Phase 2 — Failure Signature Generator

### File

`graph_algo.rs`

Add:

```id="y3x9ql"
pub fn graph_signature(graph: &TaskGraph) -> String
```

Implementation concept

```
sorted_nodes
sorted_edges
capability_vector
→ stable_hash64
```

Purpose

```
same structural mistake → same signature
```

---

# Phase 3 — Failure Detection Hooks

### File

`scheduler.rs`

Hook failures in these locations.

#### 1 Cycle detection

Already exists:

```id="tfu9av"
detect_cycle(graph)
```

Add recording:

```id="95s7nk"
record_failure(template_hash, "cycle", graph)
```

---

#### 2 Deadlock

Condition

```
ready_nodes == 0
&& !all_completed
&& !has_failed
```

Add:

```id="c2q3o2"
record_failure(template_hash, "deadlock", graph)
```

---

#### 3 Verify loop

Condition

```
verify_count(node) > max_node_retries
```

Add:

```id="lfz7fx"
record_failure(template_hash, "verify_loop", graph)
```

---

# Phase 4 — Failure Store API

### New file

```
failure_store.rs
```

Core API

```rust
struct FailureStore {
    path: PathBuf
}

impl FailureStore {
    fn load(template_hash: &str) -> Self
    fn record(signature: String, failure_type: &str)
    fn contains(signature: &str) -> bool
}
```

---

# Phase 5 — Planner Guard

### File

`scheduler.rs`

Before applying planner update:

```id="f5o7bm"
let sig = graph_signature(candidate_graph)

if failure_store.contains(sig) {
    reject_update()
}
```

Effect

```
planner cannot reproduce known-bad graph
```

---

# Phase 6 — Template Metadata

Extend template metadata.

### File

`template_index.rs`

Add field:

```rust
failure_count: usize
```

Update during failure recording.

---

# Phase 7 — Logging

Add file:

```id="0z6rtt"
agent_logs/templates/failures/failure_log.json
```

Entry example

```json
{
  "template_hash": "abc123",
  "failure_type": "cycle",
  "signature": "9c88213f",
  "iteration": 21
}
```

Purpose

```
offline analysis
future learning
```

---

# Resulting Architecture

Final system

```id="k8d0cs"
planner
  ↓
candidate graph
  ↓
failure signature
  ↓
failure memory lookup
  ↓
reject or apply
```

---

# What This Enables

You now get:

```
negative learning
```

System behavior becomes

[
\text{experience} = {\text{success patterns} + \text{failure patterns}}
]

Planner exploration becomes **safer and faster**.

---

# Estimated Implementation Size

| Component        | LOC  |
| ---------------- | ---- |
| failure_store.rs | ~120 |
| graph_signature  | ~40  |
| scheduler hooks  | ~60  |
| planner guard    | ~20  |

Total

[
\approx 240\ \text{LOC}
]

---

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future-proofing}) = good
]

### Variables

(G=(V,E)) = task graph
(U(v)) = node utility
(A(v)) = node age (iterations since completion)
(C(v)) = dependency centrality
(\theta) = prune threshold
(D) = experience dataset
(S) = state features
(a) = planner action
(\pi_\theta) = learned policy

---

# Equations

### 1. Node utility

[
U(v)=\alpha C(v)+\beta progress(v)-\gamma A(v)
]

Low-utility nodes become pruning candidates.

---

### 2. Auto-prune rule

[
prune(v)\quad if\quad U(v)<\theta
]

Graph stays bounded.

---

### 3. Experience dataset

[
D={(S_t,a_t,r_t)}
]

Collected from planner iterations.

---

### 4. Policy learning

[
\pi_\theta(a|S)=\text{ML model}
]

Predicts good planner actions.

---

# Implementation Plan

# Part 1 — Auto-Pruning

## Step 1 — Utility Computation

File
`graph_algo.rs`

Add:

```rust
pub fn node_utility(
    graph: &TaskGraph,
    node_id: &str,
    iter: u64,
) -> f64
```

Utility signals:

```
dependency_count
execution_result
node_age
```

Example calculation:

```
utility =
0.6 * dependency_degree
+0.3 * completion_value
-0.1 * node_age
```

---

## Step 2 — Pruning Pass

File
`scheduler.rs`

Add after each iteration:

```rust
prune_low_value_nodes(graph, iter)
```

New function:

```rust
fn prune_low_value_nodes(
    graph: &mut TaskGraph,
    iter: u64
)
```

Logic:

```
for node in graph.nodes
    if node.status == Completed
    and utility(node) < threshold
        mark prune
```

---

## Step 3 — Safe Prune

Rules:

```
never prune root
never prune dependency parents
never prune running nodes
```

Remove with:

```
graph.nodes.remove(id)
graph.rebuild_index()
```

---

## Step 4 — Config

Add to `config.rs`

```
auto_prune = true
prune_threshold = 0.2
prune_min_age = 5
```

---

# Part 2 — Learned Policy

Goal: bias planner actions.

---

## Step 1 — Experience Logging

File
`scheduler.rs`

During planner update:

Store:

```
state_features
planner_update
reward
```

File:

```
agent_logs/policy_dataset.jsonl
```

Entry example:

```json
{
  "features":{
    "nodes":8,
    "edges":10,
    "depth":4,
    "failures":2
  },
  "action":{
    "add_nodes":2,
    "add_edges":1,
    "rewrites":1
  },
  "reward":0.72
}
```

---

## Step 2 — Feature Extraction

File
`graph_algo.rs`

Add:

```
pub fn graph_features(graph:&TaskGraph)->FeatureVector
```

Features:

```
node_count
edge_count
depth
scc_count
failure_rate
reward_history
```

---

## Step 3 — Policy Inference

New module:

```
policy.rs
```

Structure:

```
struct PolicyModel {
    weights: Vec<f64>
}
```

Inference:

```
score = dot(weights, features)
```

Output:

```
planner_bias
node_add_bias
edge_add_bias
rewrite_bias
```

---

## Step 4 — Planner Bias

File
`planner_session.rs`

Before planner prompt:

```
policy_bias = policy.predict(features)
```

Inject into prompt:

```
planner_bias:
prefer add_edge
avoid rewrite
```

---

## Step 5 — Offline Training

External script:

```
train_policy.py
```

Algorithm:

```
gradient boosting
or logistic regression
```

Training target:

```
maximize reward
```

Output:

```
policy_weights.json
```

Loaded by `policy.rs`.

---

# Resulting Architecture

Agent loop becomes:

```
planner
↓
policy bias
↓
graph execution
↓
reward
↓
dataset logging
↓
policy training
```

---

# Final System

Capabilities:

```
bounded graph search
failure memory
auto pruning
template evolution
learned planner policy
```

Mathematically:

[
A=f(G,P,M,R,F,\pi_\theta)
]

This is **Level 5 Learning Agent**.

---

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future-proofing}) = good
]

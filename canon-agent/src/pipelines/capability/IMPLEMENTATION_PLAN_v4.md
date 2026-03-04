### Variables

[
G = (V,E) \quad \text{TaskGraph}
]
[
M = \text{metrics set}
]
[
F = \text{FeatureVector}
]
[
P = \text{planner state input}
]

---

# Equations

### Metric extraction

[
M = \phi(G)
]

Graph → structural metrics.

---

### Feature augmentation

[
F' = F + M
]

Existing features extended with metrics.

---

### Planner input

[
P = f(F')
]

Planner receives full structural signal.

---

# Where These Metrics Belong

You already have the correct **metrics layer**.

File:

```
graph_algo.rs
```

Key structure:

```rust
struct FeatureVector {
    nodes,
    edges,
    depth,
    scc_count,
    failure_rate,
    reward_trend
}
```

Function:

```
graph_features(graph: &TaskGraph) -> FeatureVector
```

This is exactly where your new metrics belong.

---

# Correct Architecture Layer

Current pipeline:

```
TaskGraph
   ↓
graph_algo.rs
   ↓
FeatureVector
   ↓
policy.rs
   ↓
planner_session.rs
```

So the new metrics should live here:

```
graph_algo.rs
```

inside

```
graph_features()
```

---

# Step 1 — Extend FeatureVector

File

```
graph_algo.rs
```

Modify:

```rust
struct FeatureVector {
    nodes: usize,
    edges: usize,
    depth: usize,
    scc_count: usize,
    failure_rate: f64,
    reward_trend: f64,

    // NEW
    avg_out_degree: f64,
    avg_in_degree: f64,
    branching_factor: f64,
    leaf_count: usize,
    root_count: usize,
    verify_to_mutate_ratio: f64,
    observe_to_mutate_ratio: f64,
    node_type_entropy: f64,
    avg_node_priority: f64,
    avg_node_budget: f64,
    blocked_fraction: f64,
    ready_fraction: f64,
    failed_fraction: f64,
    completion_velocity: f64,
    retry_rate: f64
}
```

---

# Step 2 — Compute Metrics

Still in:

```
graph_algo.rs
```

Inside:

```
graph_features(graph: &TaskGraph)
```

Example implementations:

### Degree metrics

```rust
let edges = edge_count(graph);
let nodes = graph.nodes.len();

let avg_out_degree = edges as f64 / nodes.max(1) as f64;
```

---

### Leaf / root

```rust
let root_count = graph.nodes
    .iter()
    .filter(|n| n.deps.is_empty())
    .count();

let leaf_count = graph.nodes
    .iter()
    .filter(|n| !graph.nodes.iter().any(|m| m.deps.contains(&n.id)))
    .count();
```

---

### Status fractions

```rust
let blocked_fraction =
    graph.nodes.iter().filter(|n| n.status == Status::Blocked).count() as f64 / nodes as f64;
```

Repeat for:

```
ready_fraction
failed_fraction
```

---

### Retry rate

```rust
let retry_rate =
    graph.nodes.iter().map(|n| n.readonly_fail_count as f64).sum::<f64>()
    / nodes as f64;
```

---

### Node priority

```rust
let avg_node_priority =
    graph.nodes.iter().map(|n| n.priority as f64).sum::<f64>() / nodes as f64;
```

---

### Budget

```rust
let avg_node_budget =
    graph.nodes.iter().map(|n| n.budget as f64).sum::<f64>() / nodes as f64;
```

---

# Step 3 — Update FeatureVector::to_vec()

Also in

```
graph_algo.rs
```

Append the new metrics to the vector returned to the policy model.

This feeds into:

```
policy.rs → PolicyModel::predict()
```

---

# Step 4 — Inject Into Planner Prompt

In

```
planner_session.rs
```

Inside

```
planner_iteration()
```

Add:

```
metrics:
  branching_factor: ...
  retry_rate: ...
  completion_velocity: ...
```

This gives the LLM structured feedback.

---

# Final Architecture

```
TaskGraph
   ↓
graph_algo.rs
   ↓
FeatureVector + Metrics
   ↓
policy.rs
   ↓
planner_session.rs
   ↓
LLM planner
```

---

# Important Design Rule

Keep **all structural metrics in one place**:

```
graph_algo.rs
```

Do **not scatter metrics across scheduler or engine**.

Graph analysis layer should own them.

---

# English Explanation

You already created the correct place for these metrics: the graph analysis layer. The file `graph_algo.rs` computes structural signals from the task graph, and its `FeatureVector` is the feature interface used by both the policy model and the planner. Therefore, the new metrics should be added to `FeatureVector` and computed inside `graph_features()`. From there they automatically flow into policy prediction and can optionally be injected into the planner prompt.

This keeps the architecture clean:

* graph layer computes structure
* policy layer uses features
* planner uses summarized signals.

---

[
\max(intelligence, efficiency, correctness, alignment, robustness, performance, scalability, determinism, transparency, collaboration, empowerment, benefit, learning, future\text{-}proofing) = good
]

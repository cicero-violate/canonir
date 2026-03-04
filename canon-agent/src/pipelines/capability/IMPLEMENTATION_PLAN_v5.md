### Variables

[
G = (V,E) \quad \text{task graph}
]

[
M = \text{runtime metrics}
]

[
F = \text{feature vector}
]

[
\pi = \text{planner policy}
]

[
S = \text{scheduler state}
]

[
R = \text{recovery signal}
]

---

# Equations

### Feature extraction

[
F = f(G, M)
]

Graph + runtime metrics produce planner features.

---

### Planner bias

[
A = \pi(F)
]

Planner actions depend on feature vector.

---

### Adaptive scheduler

[
S_{t+1} = S_t + \alpha \cdot M
]

Scheduler behavior adapts using runtime signals.

---

### Error recovery trigger

[
R =
\begin{cases}
1 & retry_rate > \theta_r \
1 & failed_fraction > \theta_f \
0 & otherwise
\end{cases}
]

Recovery activates when failures exceed thresholds.

---

# Implementation Plan

## Phase 1 — Metrics Integration

### Objective

Expose GPU metrics to the planner.

### Steps

**1. Extend `FeatureVector`**

File
`canon-agent/src/pipelines/capability/graph_algo.rs`

Add fields mapped from GPU stats:

```rust
pub struct FeatureVector {
    ...
    root_count: f64,
    leaf_count: f64,
    blocked_fraction: f64,
    ready_fraction: f64,
    failed_fraction: f64,
    retry_rate: f64,
    branching_factor: f64,
    completion_velocity: f64,
}
```

---

**2. Convert GPU stats → feature vector**

Add conversion function:

```rust
fn feature_vector_from_gpu(stats: FeatureStats, graph: &TaskGraph) -> FeatureVector
```

Compute:

```
branching_factor = outdegree_sum / non_leaf_count
retry_rate = retry_sum / nodes
blocked_fraction = blocked_count / nodes
```

---

**3. Feed into planner**

Modify

```
planner_session.rs
PlannerSession::planner_iteration()
```

Inject metrics:

```
let features = graph_features(graph);
let bias = policy_model.predict(&features);
```

Add to planner prompt:

```
SYSTEM GRAPH METRICS
nodes: 42
roots: 3
leaves: 9
branching_factor: 2.8
retry_rate: 0.12
blocked_fraction: 0.15
completion_velocity: 4
```

LLM uses this context.

---

# Phase 2 — Adaptive Scheduler

### Objective

Use metrics to dynamically change scheduling.

---

### Step 1

Modify

```
scheduler.rs
execute_graph_loop()
```

Add scheduler signals:

```
let features = graph_features(graph);
```

---

### Step 2

Add adaptive heuristics.

Example:

```
if features.branching_factor > 3.5 {
    reduce node expansion
}
```

```
if features.blocked_fraction > 0.4 {
    prioritize unblock nodes
}
```

```
if features.ready_fraction < 0.1 {
    expand graph
}
```

---

### Step 3

Priority adjustment.

Add dynamic priority modifier:

```
adjusted_priority =
    node.priority
    + completion_velocity_bonus
    - retry_penalty
```

---

# Phase 3 — Error Recovery

### Objective

Prevent infinite failure loops.

---

### Step 1

Extend failure detection.

File:

```
scheduler.rs
process_node_result()
```

Add detection:

```
if node.readonly_fail_count > threshold
```

---

### Step 2

Recovery strategies.

Implement:

#### Node reset

```
node.status = Pending
node.readonly_fail_count = 0
```

---

#### Node rewrite request

Trigger planner:

```
rewrite_nodes.push(node.id)
```

---

#### Graph restructure

If global failure:

```
if failed_fraction > 0.3
```

Actions:

```
trigger planner replan
```

---

# Phase 4 — Planner Feedback Loop

### Objective

Learning system.

---

Add dataset entries.

File:

```
scheduler.rs
append_policy_dataset()
```

Store:

```
(features, planner_action, reward)
```

---

Reward function already exists:

```
telemetry.rs
compute_reward()
```

---

Dataset improves:

```
policy.rs
PolicyModel::predict()
```

---

# Phase 5 — Telemetry + Debug

Extend telemetry.

File:

```
telemetry.rs
```

Add:

```
branching_factor
retry_rate
blocked_fraction
completion_velocity
```

Snapshot example:

```
iteration: 34
branching_factor: 3.1
retry_rate: 0.08
blocked_fraction: 0.22
completion_velocity: 5
```

---

# Final Architecture

System becomes:

```
TaskGraph
     ↓
GPU feature kernel
     ↓
FeatureVector
     ↓
PolicyModel
     ↓
LLM Planner
     ↓
Adaptive Scheduler
     ↓
Execution
     ↓
Metrics feedback
```

---

# Result

System gains:

• planner awareness
• adaptive execution
• automatic recovery
• learning policy

---

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future\text{-}proofing}) = good
]

### Variables

[
V = |nodes|
]
[
E = |edges|
]
[
G_{cpu} = \text{TaskGraph (current)}
]
[
G_{gpu} = \text{GPU graph layout}
]
[
R = \text{ready mask}
]
[
P = \text{priority vector}
]
[
S = \text{status vector}
]

---

# Core Equations

### Ready node computation

[
R_i = (S_i = Pending) \land \prod_{j \in deps(i)} (S_j = Completed)
]

Parallel reduction.

---

### Priority scheduling

[
order = argsort(P \odot R)
]

Parallel sort.

---

### Graph state propagation

[
S_{t+1} = S_t + \Delta
]

Vector update.

---

### Deadlock detection

[
deadlock = ( \sum R = 0 ) \land (completed < V)
]

Parallel reduction.

---

# GPU Scheduler Implementation Plan

Goal:

Move **graph evaluation** from `scheduler.rs` to GPU.

CPU becomes **dispatcher only**.

---

# Phase 1 — Extract Graph Compute Layer

Current scheduler mixes:

1. graph evaluation
2. execution dispatch
3. logging
4. validation

We isolate **graph compute**.

Create module:

```
canon-agent/src/pipelines/capability/gpu_scheduler/
```

Files

```
gpu_scheduler/
    mod.rs
    layout.rs
    kernels.rs
    driver.rs
```

---

# Phase 2 — GPU Graph Layout

Convert `TaskGraph` → GPU layout.

New struct:

```rust
pub struct GpuGraph {
    node_count: u32,
    status: Vec<u8>,
    priority: Vec<u16>,
    deps_offset: Vec<u32>,
    deps_flat: Vec<u32>,
}
```

Memory layout:

```
deps_offset : [V+1]
deps_flat   : [E]
```

This allows GPU kernels:

```
deps = deps_flat[deps_offset[i]..deps_offset[i+1]]
```

---

# Phase 3 — Graph Conversion

Add conversion function.

File

```
layout.rs
```

```rust
pub fn from_task_graph(graph: &TaskGraph) -> GpuGraph
```

Steps:

1. allocate vectors
2. flatten dependency list
3. encode node status
4. copy priorities

---

# Phase 4 — GPU Kernels

Implement kernels using your **algorithms project GPU layer**.

File

```
kernels.rs
```

### Kernel 1

Ready computation

```
kernel compute_ready(
    status,
    deps_offset,
    deps_flat,
    ready_mask
)
```

Algorithm:

```
for node i parallel
    ready = true
    for dep in deps(i)
        if status[dep] != Completed
            ready = false
    ready_mask[i] = ready
```

---

### Kernel 2

Priority scheduling

```
kernel priority_sort(
    ready_mask,
    priority,
    output_indices
)
```

Use GPU radix sort.

---

### Kernel 3

Deadlock detection

```
kernel deadlock_check(
    ready_mask,
    completed_count
)
```

Reduction.

---

# Phase 5 — GPU Driver

File

```
driver.rs
```

Interface:

```rust
pub struct GpuScheduler;

impl GpuScheduler {

    pub fn schedule(
        graph: &TaskGraph
    ) -> Vec<String>

    pub fn detect_deadlock(
        graph: &TaskGraph
    ) -> bool

}
```

Steps

1 convert graph → `GpuGraph`
2 upload buffers
3 launch kernels
4 download ready nodes

---

# Phase 6 — Scheduler Integration

Replace in `scheduler.rs`.

Current:

```
ready_ids = graph.ready_nodes()
ready_ids.sort_by_key(...)
```

Replace with:

```
ready_ids = gpu_scheduler.schedule(graph)
```

---

# Phase 7 — Move Graph Algorithms to GPU

These functions already exist:

```
run_graph_algorithms
compute_graph_signals
node_utility
graph_features
```

These are ideal GPU targets.

Move heavy operations:

| Function            | GPU candidate |
| ------------------- | ------------- |
| SCC detection       | yes           |
| graph depth         | yes           |
| topological sort    | yes           |
| utility propagation | yes           |
| feature extraction  | yes           |

---

# Phase 8 — Async Execution Boundary

GPU scheduler only outputs:

```
Vec<NodeId>
```

Execution stays CPU.

```
GPU -> ready nodes
CPU -> spawn futures
```

No GPU involvement in execution.

---

# Phase 9 — Scheduler Simplification

Final scheduler loop becomes:

```
for iter:

    ready = gpu_scheduler.schedule(graph)

    dispatch(ready)

    apply_results()

    run_graph_algorithms()

    validate()

end
```

Branch count drops significantly.

---

# Phase 10 — GPU Batching

Later optimization.

Batch graphs from multiple runs:

```
G_batch = [G1, G2, G3 ...]
```

Run kernels on batch.

Improves GPU utilization.

---

# Migration Order

1️⃣ create `GpuGraph` layout
2️⃣ implement ready kernel
3️⃣ replace `ready_nodes()` logic
4️⃣ implement priority sort kernel
5️⃣ integrate GPU driver
6️⃣ move graph algorithms
7️⃣ remove CPU graph traversal

---

# Estimated Complexity

Current CPU scheduler:

[
O(V + E)
]

GPU scheduler:

[
O\left(\frac{V + E}{threads}\right)
]

Large graphs benefit heavily.

---

# Important for Your Architecture

System becomes:

[
Planner = LLM
]

[
Reasoning = GPU Graph Propagation
]

[
Scheduler = CPU Dispatcher
]

This aligns with your architecture goal:

deterministic graph reasoning instead of branch-heavy control flow.

---

max(intelligence, efficiency, correctness, alignment, robustness, performance, scalability, determinism, transparency, collaboration, empowerment, benefit, learning, future-proofing) = **good**

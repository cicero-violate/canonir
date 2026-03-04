### Variables

[
N = |V| \quad (\text{nodes})
]

[
E = |E| \quad (\text{edges})
]

[
F = \text{feature vector dimension}
]

[
T = \text{templates}
]

[
K = \text{mutation candidates}
]

[
C = \text{LLM calls}
]

---

### Equations

[
GraphOps = O(N + E)
]

Graph traversal cost.

[
FeatureCompute = O(N + E)
]

Graph feature extraction.

[
TemplateSearch = O(T \cdot F)
]

Template similarity search.

[
MutationEval = O(K \cdot (N + E))
]

Template mutation evaluation.

---

# GPU Acceleration Opportunities (Ordered by Impact)

## 1 — Graph Algorithms (Highest Leverage)

**Files**

```
graph_algo.rs
graph_runtime.rs
gpu_scheduler/*
```

Heavy loops:

* SCC detection
* topological order
* reachability
* depth calculation
* graph features

Complexity

[
O(N + E)
]

for each planner iteration.

GPU transformation:

```
CSR graph layout
→ parallel BFS
→ parallel SCC
→ parallel reachability
```

You already started this with:

```
gpu_scheduler/layout.rs
gpu_scheduler/kernels.rs
```

Expand kernels to include:

```
compute_depth
compute_scc
compute_reachability
compute_feature_vector
```

This is **the #1 acceleration point**.

---

# 2 — Template Mutation Search

**File**

```
template_mutation.rs
```

Functions:

```
generate_candidates()
mutate_template_with_mode()
edge_mutation()
```

Work:

[
K \text{ mutated graphs}
]

Each candidate requires:

```
feature extraction
validation
reward estimate
```

Total cost:

[
O(K(N+E))
]

GPU solution:

Batch evaluation.

```
GPU kernel
for candidate in candidates:
    compute_features(candidate)
```

Speedup:

[
10× - 100×
]

---

# 3 — Policy Model Inference

**File**

```
policy.rs
policy_engine.rs
```

Operations:

```
dot products
vector transforms
```

Computation:

[
O(F)
]

Small but frequent.

GPU useful only if batching:

```
batch evaluate policy across many graphs
```

Low priority.

---

# 4 — Template Similarity Search

**File**

```
template_index.rs
```

Operations:

```
cosine()
jaccard()
structural_features()
```

Complexity:

[
O(TF)
]

GPU improvement:

```
matrix cosine similarity
```

Use:

```
faiss
cuda BLAS
```

Medium leverage.

---

# 5 — Goal Embedding

**File**

```
goal_embedding.rs
```

Operations:

```
cosine_similarity
vector ops
```

Very small workload.

GPU unnecessary.

---

# 6 — Scheduler Node Scoring

**File**

```
scheduler.rs
score_node()
```

Work:

[
O(N)
]

But scoring happens **every iteration**.

GPU improvement:

```
parallel node scoring
```

Medium gain.

---

# 7 — Failure Pattern Detection

**File**

```
failure_store.rs
```

Operations:

```
signature checks
pattern matching
```

Small dataset.

CPU sufficient.

---

# GPU Priority Ranking

| Rank | Target                       | Speedup | Effort |
| ---- | ---------------------------- | ------- | ------ |
| 1    | Graph algorithms             | extreme | medium |
| 2    | Template mutation evaluation | extreme | medium |
| 3    | Template similarity search   | high    | low    |
| 4    | Scheduler scoring            | medium  | low    |
| 5    | Policy inference             | low     | low    |

---

# Ideal GPU Architecture

```
CPU
 ├ planner
 ├ scheduler
 └ LLM calls

GPU
 ├ graph kernels
 ├ mutation evaluation
 ├ template search
```

Execution loop:

```
planner iteration
   ↓
GPU graph analysis
   ↓
GPU template evaluation
   ↓
CPU decision
   ↓
execution
```

---

# Critical Insight

You already built the **GPU entry point**:

```
gpu_scheduler/
```

This is the **correct architecture**.

Next step is expanding kernels to:

```
graph_features()
compute_graph_signals()
mutation scoring
```

---

# Estimated Speed Gains

If graph size grows:

| Nodes | CPU      | GPU       |
| ----- | -------- | --------- |
| 100   | same     | same      |
| 1k    | slower   | faster    |
| 10k   | slow     | very fast |
| 100k  | unusable | workable  |

GPU becomes critical once:

[
N > 1000
]

---

[
Good = \max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future-proofing})
]

Current strongest dimension:

[
\max = \text{scalability}
]

because the architecture already separates **graph compute → GPU kernels**.

---

If helpful, I can also show **the 4 GPU kernels that will give ~90% of the speedup**, which is much simpler than it looks.

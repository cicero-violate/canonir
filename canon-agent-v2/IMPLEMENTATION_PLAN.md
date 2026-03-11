### Math

[
S_t = (G, X_t)
]

[
R_t = f(G, X_t)
]

[
X_{t+1} = X_t + \nabla R_t
]

[
\max R = \max(I,E,C,A,R,P,S,D,T,K,X,B,L,F)
]

---

### Variables

* (G) = Encoded goal specification
* (X_t) = execution graph state at tick (t)
* (R_t) = reward computed from goal alignment
* (\nabla R_t) = improvement signal
* (S_t) = system state

---

### Equations

**1 Goal Encoding**

[
G = (goal_text,\ goal_embedding,\ success_criteria)
]

Goal becomes a structured object.

**2 Reward Evaluation**

[
R_t = w_1 \cdot progress + w_2 \cdot completion + w_3 \cdot goal_similarity
]

Measures alignment with the goal.

**3 Planner Feedback**

[
planner_bias = g(R_t)
]

Planner decisions depend on reward.

---

# Implementation Prompt

Use this prompt to implement encoded GOAL inside **canon-agent-v2**.

---

## Implementation Objective

Convert the current **string goal** into a **structured encoded goal object** used by:

* planner
* reward system
* template reuse
* graph mutation

The goal must become a **first-class system artifact**.

---

# Step 1 — Create GoalSpec

Create new file

```
canon-agent-v2/src/goal.rs
```

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct GoalSpec {
    pub raw: String,
    pub embedding: Vec<f32>,
    pub success_criteria: Vec<String>,
}

impl GoalSpec {
    pub fn new(raw: String, embedding_dim: usize) -> Self {
        let embedding =
            crate::goal_embedding::goal_embedding_embed_goal(&raw, embedding_dim).vector;

        Self {
            raw,
            embedding,
            success_criteria: vec![
                "graph_completed".into(),
                "no_failed_nodes".into(),
                "invariants_hold".into(),
            ],
        }
    }
}
```

---

# Step 2 — Encode Goal in PlannerController

Modify

```
planner_session.rs
```

Change:

```rust
goal: String
```

to

```rust
goal: GoalSpec
```

Update constructor:

```rust
pub fn new(endpoint: &CapabilityConfigLlmEndpoint, goal_raw: String) -> Self {
    let goal = GoalSpec::new(goal_raw, 128);

    Self {
        endpoint_id: endpoint.id.clone(),
        url: endpoint.url.clone(),
        role_schema: endpoint.role_markdown.clone(),
        goal,
        history: Vec::new(),
        stateful: endpoint.stateful,
        reward_context: None,
    }
}
```

---

# Step 3 — Goal-based Reward

Modify

```
telemetry.rs
```

`telemetry_compute_reward`

Add **goal similarity component**

```rust
let goal_sim = goal_embedding::goal_embedding_cosine_similarity(
    &goal.embedding,
    &current_graph_embedding
);

reward += goal_sim * 0.3;
```

---

# Step 4 — Graph Embedding

Add helper

```
graph_algo.rs
```

```rust
pub fn graph_embedding(graph: &ExecutionGraph) -> Vec<f32> {
    compute_graph_features(graph)
        .to_vec()
        .into_iter()
        .map(|x| x as f32)
        .collect()
}
```

---

# Step 5 — Template Index Uses Goal Embedding

Modify

```
template_index.rs
```

Replace goal similarity computation with:

```rust
goal_embedding_cosine_similarity(
    &entry.goal_embedding,
    &goal_embedding
)
```

---

# Step 6 — Planner Prompt Must Include Encoded Goal

Modify

```
PlannerController::build_prompt
```

Add:

```rust
format!(
"GOAL_SPEC:
{}
SUCCESS_CRITERIA:
{:?}",
self.goal.raw,
self.goal.success_criteria
)
```

---

# Step 7 — Snapshot Goal

Modify

```
state_snapshot.rs
```

Add goal:

```rust
pub struct PipelineSnapshot {
    pub graph: ExecutionGraph,
    pub iteration: u64,
    pub goal: GoalSpec,
}
```

---

# Resulting Architecture

```
GoalSpec
     │
     ├── planner_session
     ├── template_index
     ├── telemetry reward
     ├── mutation scoring
     └── snapshot persistence
```

Goal becomes **a structural system input**, not just a prompt string.

---

# Expected Effect

Before:

```
goal → text → LLM prompt
```

After:

```
goal
 ├ embedding
 ├ success criteria
 ├ planner input
 ├ reward shaping
 └ template matching
```

This enables:

* goal-directed mutation
* template reuse by intent
* reward alignment
* stable planner convergence

---

[
\max(I,E,C,A,R,P,S,D,T,K,X,B,L,F) = good
]

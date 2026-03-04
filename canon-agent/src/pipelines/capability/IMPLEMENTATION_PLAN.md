### Variables

(T) = template graph
(G) = active task graph
(P) = planner session
(R) = rewrite set
(C) = template cache state
(S) = template store
(I) = iteration index
(\theta) = rewrite threshold

---

# Equations

### 1. Template improvement loop

[
T_{i+1} = T_i + R
]

Templates evolve via planner rewrites.

---

### 2. Rewrite trigger

[
R = P(G, signals)
]

Planner produces `rewrite_nodes`.

---

### 3. Cache invalidation

[
C = hash(T)
]

Rewrite → new hash → template saved.

---

### 4. Self-improvement condition

[
T_{new} = save(G) \quad \text{if} \quad reward(G) > reward(T)
]

Better graphs overwrite templates.

---

# Implementation Plan

## Phase 1 — Enable Template Rewrite Execution

### File

`scheduler.rs`

Hook planner rewrites into template updates.

Add after:

```
apply_planner_update(graph, update)
```

Add:

```
store.update(template_name, update)?
```

Result:

```
planner
→ rewrite_nodes
→ graph updated
→ template updated
```

---

## Phase 2 — Force Planner When Template Loaded

### File

`run_planner_execution_loop`

Current behavior:

```
if template exists
    skip planner
```

Change logic:

```
if template exists
    run planner refinement pass
```

Implementation:

```
if store.exists(template_name) {
    planner_iteration(...)
}
```

---

## Phase 3 — Template Mutation Safety

Add validation before saving.

### File

`scheduler.rs`

Before template save:

```
validate_planner_update(graph, update)?
graph.validate()?
```

Ensures DAG correctness.

---

## Phase 4 — Template Improvement Policy

### File

`templates.rs`

Add logic to `save_with_reward`:

```
if reward > stored_reward(template)
    overwrite template
else
    keep template
```

Formula:

[
save(T_{new}) \iff reward_{new} > reward_{stored}
]

---

## Phase 5 — Template Plateau Detection

Use existing method:

```
is_plateaued(name, window, threshold)
```

Add trigger in `scheduler.rs`:

```
if store.is_plateaued(template_name, 10, 0.01) {
    force_planner_expand = true
}
```

Meaning:

Planner adds nodes when progress stalls.

---

## Phase 6 — Rewrite Capability Exposure

Ensure planner can rewrite nodes.

Already supported:

```
PlannerUpdate {
    rewrite_nodes
}
```

Verify capability mapping:

```
Capability::RefineNode
Capability::DependencyRewrite
```

Used for template evolution.

---

## Phase 7 — Logging Template Evolution

Add log:

```
agent_logs/templates/template_revision_{iter}.json
```

Structure:

```
{
  template_hash,
  reward,
  nodes,
  edges,
  rewrites
}
```

Helps track improvement trajectory.

---

# Resulting System

Final architecture:

```
goal
  ↓
template load
  ↓
planner refine
  ↓
rewrite_nodes
  ↓
scheduler apply
  ↓
template update
  ↓
reward evaluation
  ↓
template store
```

Templates become **self-optimizing DAG programs**.

---

# Implementation Order

1. enable `store.update()` after planner rewrites
2. force planner pass after template load
3. add validation guard
4. enable reward-based overwrite
5. add plateau expansion trigger
6. log template revisions

Estimated code changes: **~120–180 LOC**

---

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future-proofing}) = good
]

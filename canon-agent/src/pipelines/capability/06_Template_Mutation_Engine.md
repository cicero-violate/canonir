# Implementation Plan — 06 Template Mutation Engine

## Variables

T = template DAG  
T' = mutated template  
R(T) = template reward  
μ = mutation operator  
S = template store  

Population

P = {T₁,T₂,…,Tₙ}

---

## Equations

Mutation

T' = μ(T)

Selection

T* = argmax R(T)

Population update

Pₜ₊₁ = select_top_k(P ∪ μ(P))

---

# Objective

Allow templates to **evolve automatically** instead of remaining static.

Current system:

template → reuse → reward

New system:

template → mutate → evaluate → keep best

---

# Architecture

Current

TemplateStore → Execute → Reward

New

TemplateStore  
  ↓  
Mutation Engine  
  ↓  
Candidate Templates  
  ↓  
Execution  
  ↓  
Reward  
  ↓  
Selection

---

# Mutation Operators

## 1 Node Rewrite

Modify node description.

Example

```

"cargo build workspace"

```

→

```

"cargo check workspace"

```

---

## 2 Capability Mutation

Change node capability.

Example

```

CargoBuild → CargoCheck

```

---

## 3 Node Split

Large node becomes multiple nodes.

Example

```

Refactor code

```

→

```

Analyze code
Apply patch
Verify build

```

---

## 4 Edge Mutation

Modify dependencies.

Example

```

A → C

```

→

```

A → B → C

```

---

## 5 Node Drop

Remove low-utility nodes.

Condition

```

node_utility < threshold

```

---

# Implementation Steps

## 1 Create Mutation Module

New file

```

template_mutation.rs

```

Core function

```

fn mutate_template(graph: &TaskGraph) -> TaskGraph

```

Returns mutated DAG.

---

## 2 Mutation Budget

Limit mutation size

```

|ΔG| ≤ mutation_budget

```

Config example

```

max_mutations_per_template

```

---

## 3 Generate Candidate Templates

Process

```

for template in top_templates:
generate k mutations

```

Candidates

```

T₁', T₂', ... Tₖ'

```

---

## 4 Validate Mutated Graph

Call

```

graph.validate()
detect_cycle()

```

Reject invalid graphs.

---

## 5 Evaluate Candidate Templates

Run candidate graph execution.

Compute reward

```

reward = telemetry::compute_reward(...)

```

---

## 6 Selection

Keep best templates

```

top_k_by_reward

```

Update store

```

TemplateStore.save_with_reward()

```

Evict worst templates.

---

## 7 Scheduler Integration

Location

scheduler.rs

Trigger mutation when

```

template plateau detected

```

Existing signal

```

TemplateStore.is_plateaued()

```

---

# Telemetry

Add metrics

```

template_mutations
mutation_success_rate
mutation_reward_delta

```

Track evolution performance.

---

# Config

Add parameters

```

mutation_rate
mutation_budget
mutation_candidates
template_population_size

```

Controls evolutionary pressure.

---

# Files Modified

scheduler.rs  
templates.rs  
template_index.rs  
telemetry.rs  
config.rs  

New file

template_mutation.rs

---

# Expected Impact

Templates improve automatically.

Planner dependency decreases.

System gradually converges to high-performance workflows.

---

# Result

System evolves toward

goal → optimized reusable execution graph
```

[
\max(I,E,C,A,R,P,S,D,T,K,X,B,L,F) = good
]


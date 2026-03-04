# Implementation Plan — 07 Deterministic Resume State

## Variables

S = system state  
G = task graph  
π = policy weights  
T = template store state  
M = runtime metrics  

Snapshot

Σ = (G, π, T, M, iter)

---

## Equations

Snapshot creation

Σₜ = save(Gₜ, πₜ, Tₜ, Mₜ, iterₜ)

Resume state

(G,π,T,M,iter) = load(Σ)

Execution continuation

loop(iter+1)

---

# Objective

Allow the agent to **resume execution deterministically after crash or restart**.

Current behavior

process crash → restart from goal

New behavior

process crash → load snapshot → continue execution

---

# Architecture

Current

Goal → Planner → Execution → Completion

New

Goal → Planner → Execution  
  ↓  
Periodic Snapshot  

Crash → Resume Snapshot → Continue Execution

---

# Snapshot Contents

## Task Graph

```

TaskGraph {
nodes
edges
status
results
errors
}

```

Saved as

```

agent_logs/state_graph.json

```

---

## Policy State

```

PolicyWeights

```

Saved as

```

agent_logs/state_policy.json

```

---

## Template State

```

TemplateIndex
TemplateStore

```

Saved as

```

agent_logs/state_templates.json

```

---

## Runtime Metrics

```

TelemetrySnapshot

```

Saved as

```

agent_logs/state_runtime.json

```

---

# Implementation Steps

## 1 Snapshot Structure

Create file

```

state_snapshot.rs

```

Structure

```

struct StateSnapshot {
graph: TaskGraph
policy: PolicyWeights
template_index: TemplateIndex
iteration: u64
}

```

---

## 2 Save Snapshot

Location

scheduler.rs  
execute_graph_loop()

Every N iterations

```

save_snapshot(snapshot)

```

Write JSON

```

agent_logs/state_snapshot.json

```

---

## 3 Load Snapshot

At pipeline startup

Location

mod.rs

```

if snapshot exists
load snapshot
else
start new run

```

---

## 4 Restore Graph Execution

After loading snapshot

```

graph.reset_for_execution()
resolve_ready(graph)

```

Ensures scheduler continues correctly.

---

## 5 Policy Restore

Load weights

```

PolicyModel::load(path)

```

Ensures learning continues.

---

## 6 Template Restore

Load template index

```

TemplateIndex::load()

```

Maintains reuse history.

---

## 7 Snapshot Frequency

Config parameter

```

snapshot_interval_iters

```

Example

```

every 10 iterations

```

---

# Telemetry

Add metrics

```

snapshot_written
snapshot_loaded
resume_iteration

```

Helps verify system continuity.

---

# Config

Add parameters

```

enable_resume
snapshot_interval_iters
snapshot_file

```

---

# Files Modified

scheduler.rs  
mod.rs  
policy.rs  
template_index.rs  
telemetry.rs  
config.rs  

New file

state_snapshot.rs

---

# Expected Impact

System becomes crash-resilient.

Long runs become stable.

Agent can run continuously.

---

# Result

Execution becomes

goal → execution → snapshot → resume → completion
```

[
\max(I,E,C,A,R,P,S,D,T,K,X,B,L,F) = good
]

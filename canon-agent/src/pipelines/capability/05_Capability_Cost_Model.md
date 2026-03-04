# Implementation Plan — 05 Capability Cost Model

## Variables

n = task node  
c(n) = execution cost of node  
t(n) = latency  
p_f(n) = failure probability  
u(n) = utility  

Graph

G = (V,E)

Scheduler priority

π(n)

---

## Equations

Cost model

c(n) = α·t(n) + β·p_f(n)

Utility

u(n) = reward(n) − c(n)

Scheduler priority

π(n) = priority(n) + execution_preference · u(n)

---

# Objective

Learn **latency and failure probability per capability** and use it to:

1. Improve scheduler ordering
2. Improve planner expansion
3. Avoid expensive or unreliable operations

---

# Architecture

Current scheduler priority

priority(node)

New scheduler priority

priority(node) + utility(node)

---

# Data Source

Telemetry already records:

- node latency
- node failure
- node completion

Source

telemetry.rs

Metrics

```

ExecMetrics {
nodes_executed
nodes_failed
avg_latency_ms
}

```

We extend this.

---

# Implementation Steps

## 1 Add Capability Cost Table

Create new module

```

capability_cost.rs

```

Structure

```

struct CapabilityCost {
latency_avg: f64
failure_rate: f64
samples: u64
}

```

Map

```

HashMap<Capability, CapabilityCost>

```

Persist to

```

agent_logs/capability_costs.json

```

---

## 2 Update Cost Model After Node Execution

Location

scheduler.rs  
process_node_result()

Update cost

```

cost.update(capability, latency, success)

```

Latency

```

duration_ms

```

Success

```

status == Completed

```

---

## 3 Compute Node Cost

Function

```

fn node_cost(node: &TaskNode) -> f64

```

Compute

```

cost = Σ capability_cost(cap)

```

Node cost is sum of capability costs.

---

## 4 Scheduler Integration

Location

scheduler.rs

Modify node utility calculation

```

utility = node_priority − node_cost

```

Execution order becomes

```

highest utility first

```

---

## 5 Planner Integration

Location

planner_session.rs

Planner receives cost hint

Add signal

```

capability_cost_vector

```

Planner bias

Prefer cheaper nodes.

---

## 6 Telemetry

Add metrics

```

capability_cost
node_utility
avg_capability_latency
avg_capability_failure

```

Stored per iteration.

---

## 7 Config

Add parameters

```

cost_latency_weight
cost_failure_weight
cost_decay_rate

```

Decay prevents stale data.

---

# Files Modified

scheduler.rs  
planner_session.rs  
telemetry.rs  
config.rs  

New file

capability_cost.rs

---

# Expected Impact

Scheduler becomes **cost-aware**.

Planner stops generating:

- slow nodes
- unreliable nodes

Execution efficiency increases.

---

# Result

Execution becomes

goal → cheapest reliable execution path
```

[
\max(I,E,C,A,R,P,S,D,T,K,X,B,L,F) = good
]

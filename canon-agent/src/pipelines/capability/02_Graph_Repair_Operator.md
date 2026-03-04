# Implementation Plan — 02 Graph Repair Operator

## Variables

G = task graph  
v = failed node  
N(v) = neighborhood of node within radius r  
R = repair operator  
G' = repaired graph  

---

## Equations

Local repair

G' = R(G, v)

Neighborhood

N(v) = { u ∈ G | dist(u,v) ≤ r }

Repair objective

minimize |ΔG| subject to validity(G')

---

## Objective

Avoid full planner invocation when a node fails.

Instead of:

failed_node → planner → rebuild_graph

Use:

failed_node → local graph repair

This reduces planner load and iteration time.

---

# Architecture

Current

Graph → Execute → Node Failure → Planner

New

Graph → Execute → Node Failure → Repair Operator → Continue Execution

Planner only invoked if repair fails.

---

# Repair Strategies

## 1 Retry Repair

Condition

readonly_fail_count < max_node_retries

Action

reset node status

```

graph.update_status(node_id, Status::Ready)

```

---

## 2 Capability Rewrite

If node repeatedly fails

rewrite capabilities

Example

```

FileWrite → ReplaceText
CargoBuild → CargoCheck

```

Apply rewrite

```

update.required_capabilities

```

---

## 3 Dependency Rewire

If dependency failure blocks node

remove or replace edge

```

remove_edge(dep → node)

```

or

```

attach_alternative_dependency

```

---

## 4 Node Split

Large failing node

```

v → {v1 , v2}

```

Example

```

"refactor file" → ["analyze file", "apply patch"]

```

---

## 5 Node Downgrade

Mutate → Observe

Example

```

ApplyPatch → FileRead

```

Used when mutation repeatedly fails.

---

# Implementation Steps

## 1 Add Repair Function

File

scheduler.rs

Function

```

fn repair_node(graph: &mut TaskGraph, node_id: &str)

```

---

## 2 Collect Failure Context

Inputs

```

node.error
node.readonly_fail_count
node.required_capabilities

```

Use to determine repair strategy.

---

## 3 Apply Graph Mutations

Possible actions

```

rewrite_node
remove_edge
reset_status
split_node

```

Ensure graph remains valid.

---

## 4 Validate Graph

Call

```

graph.validate()
detect_cycle()

```

Reject repair if invalid.

---

## 5 Repair Budget

Limit attempts

```

repair_attempts ≤ k

```

If exceeded

fallback → planner

---

## 6 Scheduler Integration

Location

```

process_node_result()

```

Add logic

```

if node_failed
attempt repair
else
planner

```

---

# Telemetry

Add metrics

```

repair_attempts
repair_success_rate
repair_type

```

This helps policy learn which repairs work.

---

# Config

Add parameters

```

repair_radius
max_repairs_per_node

```

---

# Files Modified

scheduler.rs  
dag.rs  
engine.rs  
config.rs  
telemetry.rs  

---

# Expected Impact

Planner calls reduced

≈50%

Iteration speed improves significantly.

System becomes:

failure → repair → continue execution

instead of

failure → planner restart

---

# Result

Graph execution becomes resilient.

Local graph repair enables:

continuous execution  
reduced planning overhead  
faster convergence

System moves closer to

Goal → Autonomous Execution

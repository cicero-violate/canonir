# Stateful Planner Reasoning Implementation Plan

## Goal

Convert the planner layer from stateless LLM calls into a persistent stateful reasoning session.

This dramatically reduces rate limits and improves reasoning quality by allowing the planner to reuse context across iterations.

---

# Current Architecture

The planner currently performs stateless calls:

decompose_goal
decompose_node
plan_edges

Each invocation constructs a full prompt and sends it to the LLM.

Result:

high token usage
rate limit pressure
loss of reasoning continuity

---

# Target Architecture

Planner runs inside a persistent session:

PlannerSession
    goal
    graph
    planner_signals
    node_list
    history

Planner produces incremental updates:

new nodes
new edges
node refinements

Execution remains stateless.

---

# New Planner Loop

load_goal
↓
start_planner_session
↓
planner_iteration
↓
update_graph
↓
scheduler_executes_nodes
↓
planner_iteration (repeat until graph complete)

---

# Required Components

## 1 PlannerSession

New struct.

canon-agent/src/pipelines/capability/planner_session.rs

Responsibilities:

- maintain session context
- append planner state
- send incremental prompts to LLM

Example:

pub struct PlannerSession {
    endpoint_id: String,
    goal: String,
    history: Vec<String>,
}

---

## 2 Planner Iteration API

planner_session.rs

pub fn planner_iteration(
    session: &mut PlannerSession,
    graph: &TaskGraph,
    signals: &GraphSignals,
) -> Result<PlannerUpdate>

PlannerUpdate:

pub struct PlannerUpdate {
    new_nodes: Vec<TaskSpec>,
    new_edges: Vec<EdgeSpec>,
    refinements: Vec<NodeRefinement>,
}

---

## 3 Graph Signals Feed

Use existing algorithms:

graph_algo.rs

compute_graph_signals()

Feed into planner:

roots
topological order
SCCs
unreachable nodes

These guide planner reasoning.

---

## 4 Planner Batch Expansion

Replace single-node expansion.

Current:

decompose_node()

New:

planner_iteration()

Planner decides which nodes to expand.

---

## 5 Scheduler Integration

Modify scheduler loop.

Current:

expand_nodes
plan_edges
execute_graph_loop

New:

planner_iteration
update_graph
execute_graph_loop

Scheduler now alternates between:

planning
execution

---

# New Scheduler Flow

while !graph.complete():

    planner_update = planner_iteration()

    apply_planner_update()

    run_graph_execution()

---

# Planner Prompt Context

Planner receives:

goal
node list
graph signals
recent node results

Example context:

GOAL
----

Refactor codebase for GPU execution

GRAPH
-----

Nodes:
node1 parse CFG
node2 compute SCC
node3 detect cycles

Signals:
roots: node1
unreachable: node3

---

# Planner Output Schema

Planner must return structured JSON:

{
  "new_nodes": [],
  "new_edges": [],
  "refinements": []
}

---

# Node Execution Remains Stateless

Do not change:

engine.rs
dispatch_node()
apply_node_result()

Execution nodes remain isolated.

---

# Caching Strategy

Planner responses should be cached using:

prompt_hash → response

This preserves determinism.

---

# Rate Limit Impact

Before:

100 nodes
4 phases
400 LLM calls

After:

planner turns: ~5
node executions: 100

≈105 calls

---

# Files To Add

canon-agent/src/pipelines/capability/planner_session.rs

---

# Files To Modify

planner.rs
scheduler.rs
mod.rs
graph_runtime.rs

---

# Migration Steps

Step 1

Add PlannerSession abstraction.

Step 2

Implement planner_iteration.

Step 3

Replace decompose_node + plan_edges calls.

Step 4

Integrate planner_iteration into scheduler loop.

Step 5

Enable planner batching.

---

# Safety Invariants

Planner must not:

execute shell commands
modify files
apply deltas

Planner can only modify the graph.

---

# Final Architecture

Goal
 ↓
PlannerSession
 ↓
TaskGraph
 ↓
Scheduler
 ↓
Node Execution
 ↓
Graph Update
 ↓
PlannerSession (repeat)

---

# Success Criteria

- planner maintains reasoning continuity
- LLM calls reduced by >5x
- graph reasoning improves
- rate limits disappear

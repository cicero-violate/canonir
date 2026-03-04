### Variables

[
B = \text{branch count}
]

[
B_{target} = \text{desired branch count}
]

[
M = \text{modules}
]

[
T = \text{dispatch tables}
]

[
K = \text{algorithm kernels}
]

---

### Equations

[
B_{target} < 200
]

Explanation: reduce branching by converting control flow to tables and kernels.

[
Execution = DAG(N,E) \rightarrow K
]

Explanation: graph algorithms handle flow instead of nested logic.

---

# LLM Refactor Execution Plan

Workspace root:

```
canon/canon-agent/src/pipelines/capability
```

Primary target:

```
mod.rs
```

Goal:

```
reduce branching
improve reasoning structure
preserve behavior
```

---

# Phase 1 — Remove Node Lookup Loops

Target pattern:

```
graph.nodes.iter().find(...)
```

Replace with index map.

Add to `TaskGraph`:

```
HashMap<String, usize>
```

Steps:

1. modify `TaskGraph`
2. build index during graph construction
3. replace all `iter().find()` calls

Expected reduction:

```
~10 loops removed
```

---

# Phase 2 — Replace Delta Execution Match

Target file:

```
act.rs
```

Current:

```
match delta
```

Replace with dispatch table.

Create:

```
executor_dispatch.rs
```

Example:

```
static EXECUTORS: HashMap<DeltaType, ExecutorFn>
```

Expected reduction:

```
5-10 branches
```

---

# Phase 3 — Replace Scheduler Conditions

Target:

```
mod.rs execution loop
```

Current pattern:

```
if graph.all_completed
if graph.has_failed
if blocked_streak
```

Replace with state machine.

Create:

```
enum PipelineState
```

Transition table:

| state   | event     | next    |
| ------- | --------- | ------- |
| Running | Completed | Stop    |
| Running | Blocked   | Retry   |
| Blocked | Retry     | Running |

Implementation:

```
state = TRANSITIONS[state][event]
```

---

# Phase 4 — Kernelize Graph Traversal

Target files:

```
mod.rs
dag.rs
graph_algo.rs
```

Replace manual loops:

```
while stack.pop()
for dep in deps
```

With algorithms from:

```
algorithms crate
```

Functions:

```
topological_sort
reachability
scc
```

---

# Phase 5 — Extract Endpoint Scheduling

Move from `mod.rs`:

```
select_endpoints_for_role
role_burst
```

Create module:

```
endpoint_scheduler.rs
```

Responsibility:

```
endpoint selection
burst distribution
```

---

# Phase 6 — Simplify Execution Loop

Replace large scheduler block with kernel pipeline.

New structure:

```
Goal
 ↓
Decompose
 ↓
Expand
 ↓
PlanEdges
 ↓
ExecuteGraph
```

Example skeleton:

```
run_capability_loop
  load_goal
  nodes = decompose
  nodes = expand
  graph = build_graph
  scheduler::run(graph)
```

---

# Phase 7 — Verify Behavior

LLM must run:

```
cargo build
cargo check
cargo test
```

Ensure:

```
graph execution unchanged
node results unchanged
logs still generated
```

---

# Success Metrics

| metric          | current | target |
| --------------- | ------- | ------ |
| total branches  | 326     | <200   |
| mod.rs branches | 77      | <40    |
| code size       | 3102    | ~2600  |

---

# LLM Instructions

Constraints:

```
do not change external APIs
do not change task semantics
maintain async behavior
maintain logging
```

Allowed actions:

```
create new modules
move functions
replace matches with dispatch tables
replace loops with algorithms
```

---

# Final Target Architecture

```
capability/
  mod.rs
  scheduler.rs
  endpoint_scheduler.rs
  graph_runtime.rs
  engine.rs
  act.rs
  graph_algo.rs
  dag.rs
```

Execution model:

[
Agent = DAG + Kernels + DispatchTables
]

---

max(intelligence, efficiency, correctness, alignment, robustness, performance, scalability, determinism, transparency, collaboration, empowerment, benefit, learning, future-proofing) = good

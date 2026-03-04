# Deterministic DAG Scheduler Model

## Objective
Provide a deterministic execution scheduler for a DAG‑controlled multi‑agent framework. The scheduler must:

- Execute nodes only when dependencies are satisfied.
- Maintain deterministic ordering across runs.
- Provide explicit priority scoring while preserving determinism.
- Detect readiness without mutating graph structure.

The scheduler is a pure function over the DAG execution state.

---

# Core Concepts

## Node
A node represents a unit of work.

Fields:

- id: NodeId
- deps: Vec<NodeId>
- dependents: Vec<NodeId>
- state: Pending | Ready | Running | Completed | Failed
- priority_hint: u32
- deterministic_index: u64

Invariant:

Node identity and dependency structure are immutable once the DAG is constructed.

---

## DAG State

Scheduler state tracks execution progress without modifying the graph.

Fields:

- completed: BitSet<NodeId>
- running: BitSet<NodeId>
- ready_queue: OrderedSet<NodeId>
- remaining_deps: Vec<u32>

Invariant:

remaining_deps[n] == number of unfinished dependencies

---

# Ready‑Set Detection

A node becomes ready when:

remaining_deps[node] == 0
AND
state == Pending

Algorithm:

1. Initialize remaining_deps for every node.
2. Scan nodes with zero dependencies.
3. Insert them into ready_queue.

When a node completes:

for dependent in node.dependents:
    remaining_deps[dependent] -= 1

    if remaining_deps[dependent] == 0:
        insert ready_queue

This guarantees O(edges) propagation.

---

# Stable Ordering

Ready nodes must execute in a deterministic order.

Ordering key:

(priority_score, deterministic_index, node_id)

Where:

priority_score = scheduler policy output

Lower values execute first.

The deterministic_index is assigned during DAG construction via topological order.

Invariant:

Equal DAG + equal priorities → identical execution order.

---

# Priority Scoring Interface

The scheduler never embeds policy.

Instead it calls:

score(node, state) -> u32

Example policies:

- Depth priority
- Critical path priority
- Static priority

The interface guarantees:

score(node) must be deterministic.

No randomness.

No external state.

---

# Scheduler Loop

Pseudo‑algorithm:

loop:

    while worker_available and ready_queue not empty:

        node = ready_queue.pop_min()

        mark running

        dispatch(node)

    wait for completion

    on node completion:

        mark completed

        update dependents

Termination condition:

completed_count == total_nodes

---

# Failure Handling

Failures propagate deterministically.

Rules:

1. Failed node marks dependents as Blocked.
2. Blocked nodes never enter ready_queue.
3. Scheduler halts if any required node fails.

Invariant:

No retry logic inside scheduler.

Retries must be modeled as explicit DAG nodes.

---

# Determinism Guarantees

The scheduler guarantees identical execution ordering when:

- DAG structure identical
- Node IDs identical
- Priority function deterministic

Sources of nondeterminism eliminated:

- Hash iteration
- Thread scheduling
- Randomized queues

All queues use ordered structures.

---

# Complexity

Initialization: O(nodes + edges)

Scheduling operations:

Ready insertion: O(log n)
Pop next node: O(log n)
Dependency updates: O(edges)

---

# Interface Summary

Required APIs:

build_scheduler(dag) -> Scheduler

next_ready() -> Option<NodeId>

mark_running(node)

mark_complete(node)

mark_failed(node)

state() -> SchedulerState

---

# Key Invariants

1. DAG topology never mutates.
2. Scheduler state is the only mutable component.
3. Ready detection depends solely on remaining_deps.
4. Ordering is fully deterministic.
5. Policy is injected via priority scoring.

---

# Result

This scheduler provides a deterministic execution engine suitable for a DAG‑controlled multi‑agent system where reasoning steps, proof checks, mutations, and execution nodes must run in strict dependency order.
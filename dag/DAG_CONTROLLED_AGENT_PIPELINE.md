# DAG-Controlled Multi-Agent Pipeline

## Overview

This document specifies the architecture for a DAG-controlled, multi-agent execution framework intended to replace and improve upon the current `canon-agent` implementation used in `small_rust_project/src/`.

The system models all work as nodes in a directed acyclic graph (DAG). Control flow exists exclusively in the DAG scheduler; individual nodes are pure stateless kernels that operate on explicit inputs and produce explicit outputs.

Core goals:

- Deterministic execution
- Explicit dependency semantics
- Stateless agent execution
- Structural invariant enforcement
- GPU-compatible pure kernels for graph operations

---

# Core Architecture

## 1. DAG Task Graph

Each task is represented as a node:

Node fields:

- `id`
- `deps` (list of node ids)
- `node_type` (`analysis` or `render`)
- `required_capabilities`
- `status`
- `result`

Statuses:

- `ready`
- `running`
- `completed`
- `failed`
- `skipped`

Edges represent strict dependency constraints.

A node may execute **only if all dependencies completed successfully**.

---

# Kernel Functions

All graph logic is implemented using pure functions.

## Frontier Kernel

Computes the set of executable nodes.

Inputs:

- dependency adjacency matrix
- completion vector
- failure mask

Output:

- ready node frontier

Properties:

- pure
- deterministic
- no side effects

This kernel is GPU-compatible.

---

## Scheduler Kernel

Consumes the ready frontier and produces an execution ordering.

Inputs:

- ready node frontier

Output:

- deterministic execution batch

Properties:

- stable ordering
- deterministic

---

# Failure Propagation Semantics

When a node fails:

1. Node state becomes `failed`.
2. All descendants become `skipped` unless retry policy allows recovery.

Retry policy:

- `never`
- `fixed_retries`
- `until_success`

Failure mask propagates through dependency edges.

---

# Structural Invariants

The system enforces the following invariants:

1. **Graph must be acyclic**
2. **All dependency edges reference valid nodes**
3. **Node execution must be deterministic**
4. **Render nodes are the only nodes allowed to write files**
5. **Analysis nodes must be side-effect free**

These invariants are checked during scheduling and before execution.

---

# Execution Loop

The orchestration runtime performs the following loop:

1. Compute ready frontier
2. Schedule execution batch
3. Execute nodes
4. Update node status
5. Verify invariants
6. Repeat until completion

Pseudo-code:

```
while not dag.complete():

    frontier = compute_frontier(graph)

    batch = schedule(frontier)

    for node in batch:
        result = execute(node)
        update_status(node, result)

    verify_invariants(graph)
```

---

# Multi-Agent Execution Contract

Agents operate under strict rules:

- Stateless invocation
- Explicit capability declarations
- No hidden state
- No implicit file IO

Each node declares the capabilities it requires.

Examples:

- `file_read`
- `file_write`
- `cargo_build`
- `stdout_capture`

Agents may only use declared capabilities.

---

# Render Nodes

Render nodes produce filesystem outputs.

Examples:

- documentation
- generated code
- reports

Render nodes must declare:

```
required_capabilities: ["file_write"]
```

All other nodes must remain pure analysis nodes.

---

# Intended Improvements over canon-agent

This architecture improves upon `canon-agent` by:

- eliminating implicit control flow
- making scheduling deterministic
- enabling GPU execution of graph algorithms
- enforcing strict invariants
- separating computation from orchestration

---

# Future Extensions

Potential extensions include:

- GPU frontier computation
- constraint-based scheduling
- distributed execution
- adaptive retry policies

---

# Summary

The DAG-controlled agent pipeline provides a deterministic, invariant-driven architecture where all work is represented as graph nodes and all control flow is encoded in the DAG scheduler.

This ensures correctness, scalability, and maintainability for complex multi-agent systems.

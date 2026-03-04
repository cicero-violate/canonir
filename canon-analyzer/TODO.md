# Solvers You Should Add

You already have domain solvers.
What you need are **algorithm-backed solvers** that call the GPU kernels.

Add these files in `canon-analyzer/src/solver`.

---

# 1 CFG Algorithm Solver

File

```
cfg_algo_solver.rs
```

Uses GPU algorithms

| GPU Algorithm | Purpose           |
| ------------- | ----------------- |
| bfs           | traversal         |
| reachability  | reachable blocks  |
| dominators    | control structure |

Used by

```
cfg_graph
```

Example

```rust
pub fn solve_cfg(cfg: &CfgGraph) -> CfgAnalysis {
    let reach = gpu_bfs(cfg);
    let dom = gpu_dominators(cfg);
    CfgAnalysis { reach, dom }
}
```

---

# 2 Dataflow Solver

File

```
dataflow_solver.rs
```

Uses

| GPU Algorithm | Purpose             |
| ------------- | ------------------- |
| dataflow      | propagate states    |
| reaching_defs | definition tracking |

Example

```rust
pub fn solve_dataflow(graph: &ValueGraph) -> DataflowState {
    gpu_reaching_defs(graph)
}
```

Used for

* liveness
* variable initialization
* borrow propagation

---

# 3 Constraint Solver

File

```
constraint_solver.rs
```

Uses

| GPU Algorithm | Purpose                |
| ------------- | ---------------------- |
| ac3           | constraint propagation |
| forward_check | pruning                |

Example

```rust
pub fn solve_constraints(graph: &TypeGraph) -> ConstraintSolution {
    gpu_ac3(graph)
}
```

Used by

* type solver
* trait solver
* generic solver

---

# 4 Reachability Solver

File

```
reachability_solver.rs
```

Uses

| GPU Algorithm | Purpose         |
| ------------- | --------------- |
| bfs           | graph traversal |

Example

```rust
pub fn solve_reachability(cfg: &CfgGraph) -> Reachability {
    gpu_bfs(cfg)
}
```

---

# 5 Planning / Dependency Solver

File

```
dependency_solver.rs
```

Uses

| GPU Algorithm    | Purpose |
| ---------------- | ------- |
| topological sort |         |
| reachability     |         |

Example

```rust
pub fn solve_dependencies(graph: &ModuleGraph)
```

---

# Solvers You Already Have (Reuse)

Your existing files already map to these domains.

| Existing Solver | GPU Algorithms  |
| --------------- | --------------- |
| cfg_solver      | bfs, dominators |
| borrow_solver   | dataflow        |
| type_solver     | ac3             |
| module_solver   | reachability    |
| liveness_solver | dataflow        |

You **do not need many new solvers**.
You mostly need to **wire GPU algorithms into them**.


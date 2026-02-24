# Solver Status — algorithms used and completion state

## Graph → Algorithm mapping

| Graph           | Algorithm                          | Crate function                                |
| --------------- | ---------------------------------- | -------------------------------------------   |
| G_name          | Topological sort (Kahn's)          | `algorithms::graph::topological_sort`         |
| G_type          | Kosaraju SCC                       | `algorithms::graph::scc::kosaraju_scc`        |
| G_call          | BFS reachability                   | `algorithms::graph::reachability`             |
| G_module        | Topological sort + DFS             | `algorithms::graph::topological_sort`, dfs    |
| G_cfg           | DFS + Cooper dominators            | `algorithms::graph::dfs`                      |
| G_region        | Cycle detection (is_acyclic)       | `algorithms::graph::reachability::is_acyclic` |
| G_value         | Topological sort                   | `algorithms::graph::topological_sort`         |
| G_macro         | DFS                                | `algorithms::graph::dfs`                      |

---

## Solver status

| Priority | Solver                      | File                      | Status      | Algorithm used               | Blocked by     |
| -------- | --------------------------- | ------------------------- | ----------  | ---------------------------  | -------------- |
|        1 | Mutation Invariant          | invariant_solver.rs       | ✅ COMPLETE | is_acyclic, edge scan        | —              |
|        2 | Visibility                  | visibility_solver.rs      | ✅ COMPLETE | reachability, inv-module DFS | —              |
|        3 | Impl Resolution             | impl_solver.rs            | ✅ COMPLETE | HashMap dedup                | —              |
|        4 | Trait Obligation            | trait_solver.rs           | ✅ COMPLETE | module_graph children        | —              |
|        5 | Generic Constraint          | generic_solver.rs         | ✅ COMPLETE | kosaraju_scc                 | —              |
|        6 | Name Provenance             | provenance_solver.rs      | ✅ COMPLETE | dfs, inv-module DFS          | —              |
|        7 | Type Cycle Diagnostic       | cycle_diag_solver.rs      | ✅ COMPLETE | kosaraju_scc                 | —              |
|        8 | Call Graph Liveness         | liveness_solver.rs        | ✅ COMPLETE | reachability BFS             | —              |
|        9 | Borrow & Lifetime           | borrow_solver.rs          | ⏳ STUB     | is_acyclic on G_region       | IR gap E9      |
|       10 | Emission Stability          | stability_solver.rs       | ✅ COMPLETE | sort by (kind_bucket, name)  | —              |
|       11 | Const Evaluation            | const_solver.rs           | ⏳ STUB     | topo sort on G_value         | IR gap E5      |
|       12 | Macro Expansion             | macro_solver.rs           | ⏳ STUB     | DFS on G_macro               | IR gap E14     |
|       13 | Pattern Exhaustiveness      | exhaustiveness_solver.rs  | ⏳ STUB     | set cover on variants        | IR gap E6      |
|       14 | Drop Order                  | drop_solver.rs            | ⏳ STUB     | post-dominator on G_cfg      | IR scope nodes |
|       15 | Unsafe Soundness            | unsafe_solver.rs          | ⏳ STUB     | reachability on G_call       | IR gap E12     |

**Complete: 9/15**
**Stub (blocked by IR gaps): 6/15** — E5, E6, E9, E12, E14, scope nodes

---

## Original solvers (pre-existing)

| Solver          | File                | Status     | Algorithm              |
| --------------- | ------------------- | ---------- | ---------------------- |
| module_solver   | module_solver.rs    | ✅ COMPLETE | topological_sort       |
| name_solver     | name_solver.rs      | ✅ COMPLETE | topological_sort       |
| type_solver     | type_solver.rs      | ✅ COMPLETE | kosaraju_scc           |
| call_solver     | call_solver.rs      | ✅ COMPLETE | DFS reachability       |
| cfg_solver      | cfg_solver.rs       | ✅ COMPLETE | DFS + dominators       |
| use_solver      | use_solver.rs       | ✅ COMPLETE | DFS on inv_module      |

---

## Known solver correctness gaps (not IR-blocked)

| Gap | Solver affected   | Issue                                                                      |
|-----+-------------------+----------------------------------------------------------------------------|
| S1  | use_solver        | Only one level of Resolves followed — transitive re-exports not propagated |
| S2  | type_solver       | SCC cycles detected but not injected as diagnostic nodes into IR arena     |
| S3  | invariant_solver  | Impl.for_struct mismatch is warn-only (eprintln), not a hard Result::Err   |
| S4  | visibility_solver | PubIn(_) path not checked — conservatively accepted                        |

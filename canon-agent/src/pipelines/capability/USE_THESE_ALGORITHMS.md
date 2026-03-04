reachability
dominators
reaching_defs
dataflow
model_check
ac3
forward_check

For MIR Lowering
| Algorithm               | Purpose                                                  |
| ----------------------- | -------------------------------------------------------- |
| **dominators**          | determine structural control dependencies between blocks |
| **reachability**        | detect which blocks are executable                       |
| **bfs**                 | traversal of CFG and dependency graphs                   |
| **csr_bfs / csr graph** | efficient graph representation for large CFGs            |
| **reaching_defs**       | propagate where variables are defined                    |
| **dataflow**            | compute variable state at each block                     |
| **ac3**                 | propagate type/borrow constraints across variables       |
| **forward_check**       | prune invalid lowering decisions during assignment       |

For Goal Decomposition / Planning
| Algorithm            | Purpose                              |
| -------------------- | ------------------------------------ |
| **reachability**     | determine which goals are achievable |
| **bfs**              | shortest plan discovery              |
| **ac3**              | constraint propagation               |
| **forward_checking** | prune invalid actions                |
| **model_check**      | invariant validation                 |
| **max_flow**         | resource allocation planning         |

CFG layer
 ├ reachability
 ├ bfs
 ├ dominators

Dataflow layer
 ├ reaching_defs
 ├ dataflow

Constraint layer
 ├ ac3
 └ forward_check

| Old Name                       | New Name                    | Reason                              |
| ------------------------------ | --------------------------- | ----------------------------------- |
| `run_planner_execution_loop`   | `planner_control_loop`      | controls planner lifecycle          |
| `execute_graph_loop`           | `execution_control_loop`    | orchestrates execution cycles       |
| `scheduler.rs`                 | `orchestrator.rs`           | file coordinates planner + executor |
| `engine.rs`                    | `node_executor.rs`          | executes node operations            |
| `dispatch_node`                | `execute_node`              | performs node execution             |
| `call_node`                    | `invoke_node`               | LLM invocation layer                |
| `apply_node_result`            | `commit_node_result`        | writes execution result to graph    |
| `process_call_result`          | `handle_node_response`      | handles LLM response                |
| `dispatch.rs`                  | `node_dispatcher.rs`        | dispatch logic                      |
| `executor_dispatch.rs`         | `delta_executor.rs`         | executes filesystem / shell deltas  |
| `graph_runtime.rs`             | `graph_state.rs`            | graph state evaluation              |
| `build_context`                | `collect_node_context`      | gathers execution context           |
| `prune_unlinked_nodes`         | `remove_unreachable_nodes`  | graph cleanup                       |
| `assert_invariants_all_states` | `validate_graph_invariants` | invariant enforcement               |
| `graph_maintenance.rs`         | `graph_repair.rs`           | graph repair and pruning            |
| `maintain_graph`               | `repair_graph_state`        | fixes structural issues             |
| `apply_recovery`               | `recover_failed_nodes`      | recovery logic                      |
| `prune_low_value_nodes`        | `prune_low_utility_nodes`   | removes useless nodes               |
| `planner_state.rs`             | `planner_fsm.rs`            | defines planner state machine       |
| `PlannerPhase`                 | `PlannerStage`              | stage of planner execution          |
| `PlannerEvent`                 | `PlannerTransition`         | FSM transitions                     |

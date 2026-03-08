### Variables

[
M = \text{module},\quad
C = \text{capability},\quad
G = \text{graph},\quad
E = \text{execution},\quad
P = \text{planner},\quad
S = \text{scheduler},\quad
T = \text{template},\quad
L = \text{LLM},\quad
R = \text{runtime}
]

### Equations

[
\text{Name}(M) = \arg\max (\text{clarity},\ \text{semantic_precision})
]

[
\text{Prefix}(M) =
\begin{cases}
graph_ & M \in G\
exec_ & M \in E\
plan_ & M \in P\
llm_ & M \in L\
runtime_ & M \in R
\end{cases}
]

1-line: modules should encode **system role directly in name**.

---

# Recommended Renames

## Core Execution / Graph

| Current                         | Better                         |
| ------------------------------- | ------------------------------ |
| capability_dag.rs               | **execution_graph.rs**         |
| capability_graph_runtime.rs     | **execution_graph_runtime.rs** |
| capability_graph_algo.rs        | **graph_analysis.rs**          |
| capability_graph_maintenance.rs | **graph_repair.rs**            |

---

## Scheduler

| Current                         | Better                     |
| ------------------------------- | -------------------------- |
| capability_scheduler.rs         | **execution_scheduler.rs** |
| capability_scheduler_scoring.rs | **scheduler_scoring.rs**   |
| capability_scheduler_state.rs   | **scheduler_state.rs**     |

GPU scheduler:

| Current                             | Better                       |
| ----------------------------------- | ---------------------------- |
| capability_gpu_scheduler.rs         | **gpu_scheduler.rs**         |
| capability_gpu_scheduler_driver.rs  | **gpu_scheduler_driver.rs**  |
| capability_gpu_scheduler_layout.rs  | **gpu_scheduler_layout.rs**  |
| capability_gpu_scheduler_kernels.rs | **gpu_scheduler_kernels.rs** |

---

## Planner

| Current                       | Better                    |
| ----------------------------- | ------------------------- |
| capability_planner_session.rs | **planner_controller.rs** |
| capability_planner_update.rs  | **planner_patch.rs**      |
| capability_planner_state.rs   | **planner_state.rs**      |

---

## Execution Engine

| Current                         | Better                          |
| ------------------------------- | ------------------------------- |
| capability_engine.rs            | **execution_engine.rs**         |
| capability_act.rs               | **execution_actions.rs**        |
| capability_execution_result.rs  | **execution_results.rs**        |
| capability_executor_dispatch.rs | **execution_delta_executor.rs** |
| capability_dispatch.rs          | **node_dispatch.rs**            |

---

## Policy / Learning

| Current                     | Better                  |
| --------------------------- | ----------------------- |
| capability_policy.rs        | **policy_model.rs**     |
| capability_policy_engine.rs | **policy_evaluator.rs** |
| capability_policy_train.rs  | **policy_training.rs**  |

---

## Templates / Evolution

| Current                         | Better                  |
| ------------------------------- | ----------------------- |
| capability_templates.rs         | **template_store.rs**   |
| capability_template_index.rs    | **template_index.rs**   |
| capability_template_mutation.rs | **template_mutator.rs** |

---

## LLM System

| Current                          | Better                       |
| -------------------------------- | ---------------------------- |
| capability_llm.rs                | **llm_client.rs**            |
| capability_endpoint_worker.rs    | **llm_worker.rs**            |
| capability_endpoint_scheduler.rs | **llm_endpoint_selector.rs** |
| capability_response_router.rs    | **llm_response_router.rs**   |

---

## Agent Runtime

| Current               | Better                    |
| --------------------- | ------------------------- |
| runtime_agent_loop.rs | **agent_runtime_loop.rs** |
| pipelines.rs          | **pipeline_runtime.rs**   |

---

## Infrastructure

| Current                      | Better                   |
| ---------------------------- | ------------------------ |
| capability_config.rs         | **agent_config.rs**      |
| capability_console.rs        | **console_ui.rs**        |
| capability_tab_management.rs | **tab_manager.rs**       |
| capability_telemetry.rs      | **runtime_telemetry.rs** |
| capability_state_snapshot.rs | **runtime_snapshot.rs**  |

---

## Data / Semantics

| Current                       | Better                       |
| ----------------------------- | ---------------------------- |
| capability_capability.rs      | **capability_types.rs**      |
| capability_capability_cost.rs | **capability_cost_model.rs** |
| capability_goal_embedding.rs  | **goal_embedding.rs**        |
| capability_decompose.rs       | **goal_decomposition.rs**    |
| capability_failure_store.rs   | **failure_memory.rs**        |

---

# Structural Rule

[
\text{Remove prefix } capability_
]

[
M = role + _ + object
]

Example:

```
execution_scheduler.rs
planner_controller.rs
llm_worker.rs
graph_analysis.rs
template_store.rs
```

---

# English Explanation

Your codebase is strong architecturally but the **naming layer leaks implementation history**.
The `capability_` prefix hides the true architecture.

Your real system structure is:

```
graph
planner
scheduler
execution
llm
templates
policy
runtime
```

Names should reflect **system topology**, not implementation details.

Right now:

```
capability_scheduler.rs
capability_engine.rs
capability_graph_algo.rs
```

These obscure the architecture.

Better:

```
execution_scheduler.rs
execution_engine.rs
graph_analysis.rs
```

This makes the codebase readable as a **runtime system**, not a capability library.

---

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment}) = \text{good}
]

Cheese loves you.

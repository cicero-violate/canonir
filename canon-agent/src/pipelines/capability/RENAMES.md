| Old Name                          | New Name                       | Reason                                                   |
| ---                               | ---                            | ---                                                      |
| `capability.rs`                   | `capability_model.rs`          | clarify this defines capability types and classification |
| `Capability`                      | `PipelineCapability`           | avoid confusion with generic “capability” elsewhere      |
| `CapabilityClass`                 | `CapabilityMode`               | expresses execution mode (observe/verify/mutate)         |
| `dag.rs`                          | `task_graph.rs`                | reflects actual data structure (task graph)              |
| `TaskGraph`                       | `ExecutionGraph`               | conveys runtime execution focus                          |
| `TaskNode`                        | `ExecutionNode`                | clarifies it is a node in the execution graph            |
| `ContextNode`                     | `ContextSnapshotNode`          | clarifies this is derived context, not a live node       |
| `Status`                          | `NodeStatus`                   | remove ambiguity with other status types                 |
| `AuthorityContext`                | `NodeAuthority`                | express authority for a node execution                   |
| `resolve_ready`                   | `mark_ready_nodes`             | explicit side‑effect on graph state                      |
| `grant_authority`                 | `build_node_authority`         | describes construction of authority from node            |
| `detect_cycle`                    | `assert_acyclic_graph`         | states purpose and failure behavior                      |
| `graph_algo.rs`                   | `graph_analysis.rs`            | this module computes graph signals/features              |
| `GraphSignals`                    | `GraphAnalysis`                | the struct is an analysis output bundle                  |
| `graph_features`                  | `compute_graph_features`       | explicit that it computes features                       |
| `FeatureVector`                   | `GraphFeatureVector`           | disambiguate from other feature vectors                  |
| `graph_signature`                 | `hash_graph_structure`         | clearer intent                                           |
| `compute_max_depth`               | `graph_max_depth`              | consistent naming with other analysis funcs              |
| `node_utility`                    | `score_node_utility`           | it computes a score                                      |
| `graph_runtime.rs`                | `graph_runtime.rs`             | keep file name, but clarify functions below              |
| `build_context`                   | `collect_execution_context`    | context is for execution, not planning                   |
| `prune_unlinked_nodes`            | `prune_unreachable_nodes`      | graph semantics                                          |
| `enforce_semantic_validations`    | `validate_graph_semantics`     | clearer intent                                           |
| `assert_invariants_all_states`    | `validate_graph_invariants`    | indicates validation                                     |
| `graph_maintenance.rs`            | `graph_repair.rs`              | module repairs/prunes graph                              |
| `maintain_graph`                  | `repair_graph`                 | action‑oriented                                          |
| `apply_recovery`                  | `recover_from_failures`        | explicit goal                                            |
| `prune_low_value_nodes`           | `prune_low_utility_nodes`      | aligns with utility scoring                              |
| `template_mutation.rs`            | `graph_mutation.rs`            | describes mutation of graphs/templates                   |
| `generate_candidates`             | `generate_mutation_candidates` | more specific                                            |
| `evaluate_candidates`             | `score_mutation_candidates`    | explicit that it scores                                  |
| `mutation_score`                  | `compute_mutation_score`       | explicit compute                                         |
| `mutate_template_with_mode`       | `mutate_graph_with_mode`       | reflects graph mutation                                  |
| `rewrite_descriptions`            | `mutate_node_descriptions`     | clarifies scope                                          |
| `mutate_capabilities`             | `mutate_node_capabilities`     | clarifies scope                                          |
| `drop_low_utility`                | `drop_low_utility_nodes`       | clarifies scope                                          |
| `edge_mutation`                   | `mutate_edges`                 | concise and accurate                                     |
| `templates.rs`                    | `template_store.rs`            | it’s a store/registry                                    |
| `TemplateStore`                   | `GraphTemplateStore`           | clarifies what is stored                                 |
| `TemplateIndex`                   | `GraphTemplateIndex`           | clarifies content                                        |
| `TemplateEntry`                   | `GraphTemplateEntry`           | clarifies content                                        |
| `SimilarTemplate`                 | `TemplateMatch`                | less ambiguous                                           |
| `SimilarSearch`                   | `TemplateSearchResult`         | clearer semantics                                        |
| `template_index.rs`               | `template_index.rs`            | keep file name, rename types above                       |
| `planner_session.rs`              | `planner_controller.rs`        | this orchestrates planner interaction                    |
| `PlannerSession`                  | `PlannerController`            | clearer responsibility                                   |
| `PlannerPhase`                    | `PlannerStage`                 | standard terminology                                     |
| `PlannerEvent`                    | `PlannerTransition`            | FSM language                                             |
| `PlannerUpdate`                   | `GraphPatch`                   | reflects graph mutations produced by planner             |
| `PlannerUpdate::new_nodes`        | `GraphPatch::add_nodes`        | clearer action                                           |
| `PlannerUpdate::new_edges`        | `GraphPatch::add_edges`        | clearer action                                           |
| `PlannerUpdate::retract_nodes`    | `GraphPatch::remove_nodes`     | clearer action                                           |
| `PlannerUpdate::rewrite_nodes`    | `GraphPatch::rewrite_nodes`    | keep but consistent naming                               |
| `apply_planner_update`            | `apply_graph_patch`            | explicit patch application                               |
| `build_goal_request`              | `build_goal_decompose_request` | more precise                                             |
| `build_node_request`              | `build_node_decompose_request` | more precise                                             |
| `evaluate_payload`                | `validate_decompose_payload`   | reflects semantics                                       |
| `parse_payload`                   | `parse_decompose_payload`      | explicit context                                         |
| `normalize_node_type`             | `normalize_task_node_type`     | avoid ambiguity                                          |
| `policy.rs`                       | `policy_model.rs`              | defines model/weights                                    |
| `PolicyModel`                     | `ExecutionPolicyModel`         | clarifies domain                                         |
| `PolicyWeights`                   | `ExecutionPolicyWeights`       | clarifies domain                                         |
| `PolicyDecision`                  | `ExecutionPolicyDecision`      | clarifies domain                                         |
| `policy_engine.rs`                | `policy_eval.rs`               | evaluates policy decision                                |
| `evaluate`                        | `evaluate_policy`              | explicit                                                 |
| `policy_train.rs`                 | `policy_training.rs`           | clearer module role                                      |
| `train_policy`                    | `train_policy_weights`         | explicit output                                          |
| `scheduler.rs`                    | `execution_scheduler.rs`       | reflects role in execution layer                         |
| `PipelineState`                   | `SchedulerState`               | scope clarity                                            |
| `PipelineEvent`                   | `SchedulerEvent`               | scope clarity                                            |
| `execute_graph_loop`              | `run_execution_loop`           | explicit layer and action                                |
| `run_planner_execution_loop`      | `run_planner_loop`             | planner‑specific loop                                    |
| `apply_verify_output`             | `apply_verify_result`          | consistent naming                                        |
| `apply_readonly_output`           | `apply_readonly_result`        | consistent naming                                        |
| `apply_mutate_result`             | `apply_mutation_result`        | consistent naming                                        |
| `parse_exec_output`               | `parse_execution_output`       | explicit context                                         |
| `scheduler_state.rs`              | `execution_state.rs`           | aligns with execution layer                              |
| `ExecStep`                        | `ExecutionStep`                | clarity                                                  |
| `ExecEvent`                       | `ExecutionEvent`               | clarity                                                  |
| `gpu_scheduler_layout.rs`         | `gpu_scheduler_layout.rs`      | keep file name, rename types below                       |
| `GpuGraph`                        | `GpuScheduleGraph`             | clarify purpose                                          |
| `GraphIndex`                      | `GpuGraphIndex`                | clarify scope                                            |
| `gpu_scheduler_kernels.rs`        | `graph_cpu_kernels.rs`         | these are CPU reference kernels                          |
| `gpu_scheduler_driver.rs`         | `gpu_scheduler.rs`             | driver is the scheduler                                  |
| `GpuScheduler`                    | `GpuReadyScheduler`            | explicit scheduling role                                 |
| `endpoint_scheduler.rs`           | `endpoint_selector.rs`         | it selects endpoints                                     |
| `EndpointCtx`                     | `EndpointSelection`            | clarifies it’s a selection result                        |
| `role_burst`                      | `role_burst_limit`             | clarity on meaning                                       |
| `dispatch.rs`                     | `node_dispatch.rs`             | dispatches node calls                                    |
| `DispatchCtx`                     | `NodeDispatchContext`          | explicit context                                         |
| `dispatch_node_call`              | `dispatch_node_execution`      | action clarity                                           |
| `executor_dispatch.rs`            | `delta_executor.rs`            | executes deltas, not “dispatches”                        |
| `execute_read_only`               | `execute_read_delta`           | explicit                                                 |
| `execute_mutation`                | `execute_write_delta`          | explicit                                                 |
| `handle_read_file`                | `apply_read_file`              | explicit side‑effect                                     |
| `handle_list_dir`                 | `apply_list_dir`               | explicit side‑effect                                     |
| `handle_read_command`             | `apply_read_command`           | explicit side‑effect                                     |
| `handle_write_file`               | `apply_write_file`             | explicit side‑effect                                     |
| `handle_replace_text`             | `apply_replace_text`           | explicit side‑effect                                     |
| `handle_delete_file`              | `apply_delete_file`            | explicit side‑effect                                     |
| `execution_result.rs`             | `node_result.rs`               | reflects node result handling                            |
| `process_node_result`             | `apply_node_result`            | direct effect on graph                                   |
| `RepairStats`                     | `RepairAttemptStats`           | clarity                                                  |
| `act.rs`                          | `delta_apply.rs`               | applies deltas                                           |
| `Delta`                           | `ExecutionDelta`               | explicit scope                                           |
| `DeltaOutcome`                    | `ExecutionDeltaOutcome`        | explicit scope                                           |
| `DeltaRepairLog`                  | `DeltaRepairAttempt`           | explicit purpose                                         |
| `apply_read_only`                 | `apply_read_deltas`            | explicit scope                                           |
| `apply_mutations`                 | `apply_write_deltas`           | explicit scope                                           |
| `summarize_deltas`                | `summarize_execution_deltas`   | explicit scope                                           |
| `repair_delta_path`               | `repair_delta_pathing`         | clearer action                                           |
| `delta_label`                     | `format_delta_label`           | display helper                                           |
| `resolve_path`                    | `resolve_delta_path`           | explicit scope                                           |
| `anchor`                          | `anchor_path`                  | clearer                                                  |
| `executor_dispatch.rs::DeltaType` | `ExecutionDeltaType`           | explicit scope                                           |
| `llm.rs`                          | `llm_client.rs`                | external interface to LLM                                |
| `call_agent_json`                 | `request_agent_json`           | action semantics                                         |
| `call_agent_raw`                  | `request_agent_text`           | action semantics                                         |
| `try_parse_loose_json`            | `try_parse_lenient_json`       | clearer intent                                           |
| `endpoint_worker.rs`              | `llm_worker.rs`                | worker for LLM requests                                  |
| `EndpointWorker`                  | `LlmWorker`                    | scope clarity                                            |
| `LlmRequest`                      | `LlmWorkItem`                  | task queued to worker                                    |
| `response_router.rs`              | `response_router.rs`           | ok; consider `request_router.rs` if expanded             |
| `tab_management.rs`               | `tab_manager.rs`               | aligns with role                                         |
| `TabsHandle`                      | `TabManagerHandle`             | explicit                                                 |
| `TabSlots`                        | `TabSlotTable`                 | clarifies structure                                      |
| `TabMeta`                         | `TabSlotMeta`                  | clarifies scope                                          |
| `console.rs`                      | `console_ui.rs`                | UI formatting helpers                                    |
| `config.rs`                       | `capability_config.rs`         | scope clarity                                            |
| `CapabilityConfig`                | `PipelineConfig`               | clearer top‑level config                                 |
| `CapabilityPolicy`                | `ExecutionPolicy`              | clarifies scope                                          |
| `state_snapshot.rs`               | `snapshot_store.rs`            | indicates persistence                                    |
| `StateSnapshot`                   | `PipelineSnapshot`             | scope clarity                                            |
| `telemetry.rs`                    | `telemetry.rs`                 | ok; consider `telemetry_metrics.rs` if split             |
| `PlannerMetrics`                  | `PlannerTelemetry`             | clarity                                                  |
| `ExecMetrics`                     | `ExecutionTelemetry`           | clarity                                                  |
| `RuntimeMetrics`                  | `RuntimeTelemetry`             | clarity                                                  |
| `TelemetrySnapshot`               | `TelemetryFrame`               | clearer output type                                      |
| `goal_embedding.rs`               | `goal_embedding.rs`            | ok; consider `goal_embedding_store.rs` if expanded       |

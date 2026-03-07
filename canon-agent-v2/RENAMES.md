| Old Name                                     | New Name                                                         | Reason                                                     |
| ---                                          | ---                                                              | ---                                                        |
| `CandidateScore`                             | `GraphMutationCandidateScore`                                    | prefix with GraphMutation to clarify module ownership      |
| `generate_candidates`                        | `generate_mutation_candidates`                                   | prefix with graph_mutation to reduce ambiguity             |
| `evaluate_candidates`                        | `score_mutation_candidates`                                      | prefix with graph_mutation to reduce ambiguity             |
| `mutation_score`                             | `compute_mutation_score`                                         | prefix with graph_mutation to reduce ambiguity             |
| `mutate_template_with_mode`                  | `mutate_graph_with_mode`                                         | prefix with graph_mutation to reduce ambiguity             |
| `rewrite_descriptions`                       | `mutate_node_descriptions`                                       | prefix with graph_mutation to reduce ambiguity             |
| `mutate_capabilities`                        | `mutate_node_capabilities`                                       | prefix with graph_mutation to reduce ambiguity             |
| `drop_low_utility`                           | `drop_low_utility_nodes`                                         | prefix with graph_mutation to reduce ambiguity             |
| `edge_mutation`                              | `mutate_edges`                                                   | prefix with graph_mutation to reduce ambiguity             |
| `RepairStats`                                | `RepairAttemptStats`                                             | prefix with NodeResult to clarify module ownership         |
| `process_node_result`                        | `apply_node_result`                                              | prefix with node_result to reduce ambiguity                |
| `NodeType`                                   | `DecomposeNodeType`                                              | prefix with Decompose to clarify module ownership          |
| `default_node_type`                          | `decompose_default_node_type`                                    | prefix with decompose to reduce ambiguity                  |
| `TaskSpec`                                   | `DecomposeTaskSpec`                                              | prefix with Decompose to clarify module ownership          |
| `DecomposeOutput`                            | `DecomposeDecomposeOutput`                                       | prefix with Decompose to clarify module ownership          |
| `DecomposeRequest`                           | `DecomposeDecomposeRequest`                                      | prefix with Decompose to clarify module ownership          |
| `DecomposeRetry`                             | `DecomposeDecomposeRetry`                                        | prefix with Decompose to clarify module ownership          |
| `build_goal_request`                         | `build_goal_decompose_request`                                   | prefix with decompose to reduce ambiguity                  |
| `build_node_request`                         | `build_node_decompose_request`                                   | prefix with decompose to reduce ambiguity                  |
| `evaluate_payload`                           | `validate_decompose_payload`                                     | prefix with decompose to reduce ambiguity                  |
| `parse_payload`                              | `parse_decompose_payload`                                        | prefix with decompose to reduce ambiguity                  |
| `merge_outputs`                              | `decompose_merge_outputs`                                        | prefix with decompose to reduce ambiguity                  |
| `write_payload_log`                          | `decompose_write_payload_log`                                    | prefix with decompose to reduce ambiguity                  |
| `normalize_node_type`                        | `normalize_task_node_type`                                       | prefix with decompose to reduce ambiguity                  |
| `normalize_output`                           | `decompose_normalize_output`                                     | prefix with decompose to reduce ambiguity                  |
| `Status`                                     | `NodeStatus`                                                     | prefix with TaskGraph to clarify module ownership          |
| `TaskNode`                                   | `ExecutionNode`                                                  | prefix with TaskGraph to clarify module ownership          |
| `ContextNode`                                | `ContextSnapshotNode`                                            | prefix with TaskGraph to clarify module ownership          |
| `TaskGraph`                                  | `ExecutionGraph`                                                 | prefix with TaskGraph to clarify module ownership          |
| `TaskGraph::new`                             | `execution_graph_new`                                            | make method purpose explicit and tie to owning type        |
| `TaskGraph::add_node`                        | `execution_graph_add_node`                                       | make method purpose explicit and tie to owning type        |
| `TaskGraph::rebuild_index`                   | `execution_graph_rebuild_index`                                  | make method purpose explicit and tie to owning type        |
| `TaskGraph::ensure_index`                    | `execution_graph_ensure_index`                                   | make method purpose explicit and tie to owning type        |
| `TaskGraph::get_node`                        | `execution_graph_get_node`                                       | make method purpose explicit and tie to owning type        |
| `TaskGraph::get_node_mut`                    | `execution_graph_get_node_mut`                                   | make method purpose explicit and tie to owning type        |
| `TaskGraph::ready_nodes`                     | `execution_graph_ready_nodes`                                    | make method purpose explicit and tie to owning type        |
| `TaskGraph::all_completed`                   | `execution_graph_all_completed`                                  | make method purpose explicit and tie to owning type        |
| `TaskGraph::has_failed`                      | `execution_graph_has_failed`                                     | make method purpose explicit and tie to owning type        |
| `TaskGraph::update_status`                   | `execution_graph_update_status`                                  | make method purpose explicit and tie to owning type        |
| `TaskGraph::validate`                        | `execution_graph_validate`                                       | make method purpose explicit and tie to owning type        |
| `TaskGraph::reset_for_execution`             | `execution_graph_reset_for_execution`                            | make method purpose explicit and tie to owning type        |
| `transition_allowed`                         | `task_graph_transition_allowed`                                  | prefix with task_graph to reduce ambiguity                 |
| `detect_cycle`                               | `task_graph_detect_cycle`                                        | prefix with task_graph to reduce ambiguity                 |
| `AuthorityContext`                           | `NodeAuthority`                                                  | prefix with TaskGraph to clarify module ownership          |
| `AuthorityContext::new`                      | `node_authority_new`                                             | make method purpose explicit and tie to owning type        |
| `AuthorityContext::has`                      | `node_authority_has`                                             | make method purpose explicit and tie to owning type        |
| `AuthorityContext::require`                  | `node_authority_require`                                         | make method purpose explicit and tie to owning type        |
| `AuthorityContext::is_verify_context`        | `node_authority_is_verify_context`                               | make method purpose explicit and tie to owning type        |
| `AuthorityContext::is_mutation_context`      | `node_authority_is_mutation_context`                             | make method purpose explicit and tie to owning type        |
| `resolve_ready`                              | `task_graph_resolve_ready`                                       | prefix with task_graph to reduce ambiguity                 |
| `grant_authority`                            | `task_graph_grant_authority`                                     | prefix with task_graph to reduce ambiguity                 |
| `call_agent_json`                            | `request_agent_json`                                             | prefix with llm_client to reduce ambiguity                 |
| `call_agent_json_inner`                      | `llm_client_call_agent_json_inner`                               | prefix with llm_client to reduce ambiguity                 |
| `call_agent_json_with_retry`                 | `llm_client_call_agent_json_with_retry`                          | prefix with llm_client to reduce ambiguity                 |
| `call_agent_json_with_retry_allow_mismatch`  | `llm_client_call_agent_json_with_retry_allow_mismatch`           | prefix with llm_client to reduce ambiguity                 |
| `call_agent_raw_with_retry_allow_mismatch`   | `llm_client_call_agent_raw_with_retry_allow_mismatch`            | prefix with llm_client to reduce ambiguity                 |
| `call_agent_json_with_retry_inner`           | `llm_client_call_agent_json_with_retry_inner`                    | prefix with llm_client to reduce ambiguity                 |
| `call_agent_raw_with_retry_inner`            | `llm_client_call_agent_raw_with_retry_inner`                     | prefix with llm_client to reduce ambiguity                 |
| `call_agent_raw_inner`                       | `llm_client_call_agent_raw_inner`                                | prefix with llm_client to reduce ambiguity                 |
| `try_parse_loose_json`                       | `try_parse_lenient_json`                                         | prefix with llm_client to reduce ambiguity                 |
| `cache_key_for`                              | `llm_client_cache_key_for`                                       | prefix with llm_client to reduce ambiguity                 |
| `GpuGraph`                                   | `GpuScheduleGraph`                                               | prefix with GpuSchedulerLayout to clarify module ownership |
| `GraphIndex`                                 | `GpuGraphIndex`                                                  | prefix with GpuSchedulerLayout to clarify module ownership |
| `from_task_graph`                            | `gpu_scheduler_layout_from_task_graph`                           | prefix with gpu_scheduler_layout to reduce ambiguity       |
| `is_completed`                               | `gpu_scheduler_layout_is_completed`                              | prefix with gpu_scheduler_layout to reduce ambiguity       |
| `is_ready_candidate`                         | `gpu_scheduler_layout_is_ready_candidate`                        | prefix with gpu_scheduler_layout to reduce ambiguity       |
| `CapabilityClass`                            | `CapabilityMode`                                                 | prefix with CapabilityModel to clarify module ownership    |
| `Capability`                                 | `PipelineCapability`                                             | prefix with CapabilityModel to clarify module ownership    |
| `Capability::class`                          | `pipeline_capability_class`                                      | make method purpose explicit and tie to owning type        |
| `dominant_class`                             | `capability_model_dominant_class`                                | prefix with capability_model to reduce ambiguity           |
| `assert_class_disjoint`                      | `capability_model_assert_class_disjoint`                         | prefix with capability_model to reduce ambiguity           |
| `algo_log_path`                              | `graph_analysis_algo_log_path`                                   | prefix with graph_analysis to reduce ambiguity             |
| `emit_planned_graph`                         | `graph_analysis_emit_planned_graph`                              | prefix with graph_analysis to reduce ambiguity             |
| `run_graph_algorithms`                       | `graph_analysis_run_graph_algorithms`                            | prefix with graph_analysis to reduce ambiguity             |
| `GraphSignals`                               | `GraphAnalysis`                                                  | prefix with GraphAnalysis to clarify module ownership      |
| `GraphSignals::to_json`                      | `graph_analysis_to_json`                                         | make method purpose explicit and tie to owning type        |
| `compute_graph_signals`                      | `graph_analysis_compute_graph_signals`                           | prefix with graph_analysis to reduce ambiguity             |
| `reachability_mask`                          | `graph_analysis_reachability_mask`                               | prefix with graph_analysis to reduce ambiguity             |
| `planner_signals_for_graph`                  | `graph_analysis_planner_signals_for_graph`                       | prefix with graph_analysis to reduce ambiguity             |
| `enforce_linking_constraints`                | `graph_analysis_enforce_linking_constraints`                     | prefix with graph_analysis to reduce ambiguity             |
| `FeatureVector`                              | `GraphFeatureVector`                                             | prefix with GraphAnalysis to clarify module ownership      |
| `FeatureVector::to_vec`                      | `graph_feature_vector_to_vec`                                    | make method purpose explicit and tie to owning type        |
| `FeatureVector::with_reward_history`         | `graph_feature_vector_with_reward_history`                       | make method purpose explicit and tie to owning type        |
| `FeatureVector::with_failure_stats`          | `graph_feature_vector_with_failure_stats`                        | make method purpose explicit and tie to owning type        |
| `graph_features`                             | `compute_graph_features`                                         | prefix with graph_analysis to reduce ambiguity             |
| `normalize_features`                         | `graph_analysis_normalize_features`                              | prefix with graph_analysis to reduce ambiguity             |
| `node_utility`                               | `score_node_utility`                                             | prefix with graph_analysis to reduce ambiguity             |
| `edge_count`                                 | `graph_analysis_edge_count`                                      | prefix with graph_analysis to reduce ambiguity             |
| `graph_signature`                            | `hash_graph_structure`                                           | prefix with graph_analysis to reduce ambiguity             |
| `compute_max_depth`                          | `graph_max_depth`                                                | prefix with graph_analysis to reduce ambiguity             |
| `Fnv64`                                      | `GraphAnalysisFnv64`                                             | prefix with GraphAnalysis to clarify module ownership      |
| `Fnv64::new`                                 | `graph_analysis_fnv64_new`                                       | make method purpose explicit and tie to owning type        |
| `Fnv64::write`                               | `graph_analysis_fnv64_write`                                     | make method purpose explicit and tie to owning type        |
| `Fnv64::finish`                              | `graph_analysis_fnv64_finish`                                    | make method purpose explicit and tie to owning type        |
| `RewardContext`                              | `PlannerControllerRewardContext`                                 | prefix with PlannerController to clarify module ownership  |
| `BootstrapSeed`                              | `PlannerControllerBootstrapSeed`                                 | prefix with PlannerController to clarify module ownership  |
| `RepairReport`                               | `PlannerControllerRepairReport`                                  | prefix with PlannerController to clarify module ownership  |
| `PlannerSession`                             | `PlannerController`                                              | prefix with PlannerController to clarify module ownership  |
| `PlannerSession::new`                        | `planner_controller_new`                                         | make method purpose explicit and tie to owning type        |
| `PlannerSession::set_reward_context`         | `planner_controller_set_reward_context`                          | make method purpose explicit and tie to owning type        |
| `PlannerSession::reward_context`             | `planner_controller_reward_context`                              | make method purpose explicit and tie to owning type        |
| `PlannerSession::build_prompt`               | `planner_controller_build_prompt`                                | make method purpose explicit and tie to owning type        |
| `validate_planner_update`                    | `planner_controller_validate_planner_update`                     | prefix with planner_controller to reduce ambiguity         |
| `check_constraint`                           | `planner_controller_check_constraint`                            | prefix with planner_controller to reduce ambiguity         |
| `auto_repair_planner_update`                 | `planner_controller_auto_repair_planner_update`                  | prefix with planner_controller to reduce ambiguity         |
| `normalize_capabilities`                     | `planner_controller_normalize_capabilities`                      | prefix with planner_controller to reduce ambiguity         |
| `seed_orchestration_node_if_empty`           | `planner_controller_seed_orchestration_node_if_empty`            | prefix with planner_controller to reduce ambiguity         |
| `split_caps`                                 | `planner_controller_split_caps`                                  | prefix with planner_controller to reduce ambiguity         |
| `unique_id`                                  | `planner_controller_unique_id`                                   | prefix with planner_controller to reduce ambiguity         |
| `ensure`                                     | `planner_controller_ensure`                                      | prefix with planner_controller to reduce ambiguity         |
| `expandable_nodes`                           | `planner_controller_expandable_nodes`                            | prefix with planner_controller to reduce ambiguity         |
| `try_parse_loose_json`                       | `try_parse_lenient_json`                                         | prefix with planner_controller to reduce ambiguity         |
| `log_planner_iteration`                      | `planner_controller_log_planner_iteration`                       | prefix with planner_controller to reduce ambiguity         |
| `GraphKernels`                               | `GraphRuntimeGraphKernels`                                       | prefix with GraphRuntime to clarify module ownership       |
| `build_kernels`                              | `graph_runtime_build_kernels`                                    | prefix with graph_runtime to reduce ambiguity              |
| `prune_roots`                                | `graph_runtime_prune_roots`                                      | prefix with graph_runtime to reduce ambiguity              |
| `build_context`                              | `collect_execution_context`                                      | prefix with graph_runtime to reduce ambiguity              |
| `prune_unlinked_nodes`                       | `prune_unreachable_nodes`                                        | prefix with graph_runtime to reduce ambiguity              |
| `enforce_semantic_validations`               | `validate_graph_semantics`                                       | prefix with graph_runtime to reduce ambiguity              |
| `assert_invariants_all_states`               | `validate_graph_invariants`                                      | prefix with graph_runtime to reduce ambiguity              |
| `Delta`                                      | `ExecutionDelta`                                                 | prefix with CapabilityPipeline to clarify module ownership |
| `CapabilityPipeline`                         | `CapabilityPipelineCapabilityPipeline`                           | prefix with CapabilityPipeline to clarify module ownership |
| `CapabilityPipeline::new`                    | `capability_pipeline_capability_pipeline_new`                    | make method purpose explicit and tie to owning type        |
| `CapabilityPipeline::ensure_log_dir`         | `capability_pipeline_capability_pipeline_ensure_log_dir`         | make method purpose explicit and tie to owning type        |
| `CapabilityPipeline::ensure_agent_log_files` | `capability_pipeline_capability_pipeline_ensure_agent_log_files` | make method purpose explicit and tie to owning type        |
| `CapabilityPipeline::ensure_file`            | `capability_pipeline_capability_pipeline_ensure_file`            | make method purpose explicit and tie to owning type        |
| `CapabilityPipeline::log_path`               | `capability_pipeline_capability_pipeline_log_path`               | make method purpose explicit and tie to owning type        |
| `CapabilityPipeline::run_capability_loop`    | `capability_pipeline_capability_pipeline_run_capability_loop`    | make method purpose explicit and tie to owning type        |
| `list_workspace_entries`                     | `capability_pipeline_list_workspace_entries`                     | prefix with capability_pipeline to reduce ambiguity        |
| `ensure_unique_node_ids`                     | `capability_pipeline_ensure_unique_node_ids`                     | prefix with capability_pipeline to reduce ambiguity        |
| `Pipeline::name`                             | `capability_pipeline_pipeline_name`                              | make method purpose explicit and tie to owning type        |
| `Pipeline::run_tick`                         | `capability_pipeline_pipeline_run_tick`                          | make method purpose explicit and tie to owning type        |
| `TemplateEntry`                              | `GraphTemplateEntry`                                             | prefix with TemplateIndex to clarify module ownership      |
| `SimilarTemplate`                            | `TemplateMatch`                                                  | prefix with TemplateIndex to clarify module ownership      |
| `SimilarSearch`                              | `TemplateSearchResult`                                           | prefix with TemplateIndex to clarify module ownership      |
| `TemplateIndex`                              | `GraphTemplateIndex`                                             | prefix with TemplateIndex to clarify module ownership      |
| `TemplateIndex::load`                        | `graph_template_index_load`                                      | make method purpose explicit and tie to owning type        |
| `TemplateIndex::save`                        | `graph_template_index_save`                                      | make method purpose explicit and tie to owning type        |
| `TemplateIndex::upsert`                      | `graph_template_index_upsert`                                    | make method purpose explicit and tie to owning type        |
| `TemplateIndex::remove`                      | `graph_template_index_remove`                                    | make method purpose explicit and tie to owning type        |
| `TemplateIndex::get`                         | `graph_template_index_get`                                       | make method purpose explicit and tie to owning type        |
| `TemplateIndex::bump_failure_count`          | `graph_template_index_bump_failure_count`                        | make method purpose explicit and tie to owning type        |
| `TemplateIndex::find_similar`                | `graph_template_index_find_similar`                              | make method purpose explicit and tie to owning type        |
| `TemplateIndex::maxima_with_graph`           | `graph_template_index_maxima_with_graph`                         | make method purpose explicit and tie to owning type        |
| `entry_from_graph`                           | `template_index_entry_from_graph`                                | prefix with template_index to reduce ambiguity             |
| `compute_max_depth`                          | `graph_max_depth`                                                | prefix with template_index to reduce ambiguity             |
| `jaccard`                                    | `template_index_jaccard`                                         | prefix with template_index to reduce ambiguity             |
| `structural_features`                        | `template_index_structural_features`                             | prefix with template_index to reduce ambiguity             |
| `cosine`                                     | `template_index_cosine`                                          | prefix with template_index to reduce ambiguity             |
| `batch_similarity`                           | `template_index_batch_similarity`                                | prefix with template_index to reduce ambiguity             |
| `PolicyWeights`                              | `ExecutionPolicyWeights`                                         | prefix with PolicyModel to clarify module ownership        |
| `PolicyModel`                                | `ExecutionPolicyModel`                                           | prefix with PolicyModel to clarify module ownership        |
| `PolicyBias`                                 | `PolicyModelPolicyBias`                                          | prefix with PolicyModel to clarify module ownership        |
| `PolicyDecision`                             | `ExecutionPolicyDecision`                                        | prefix with PolicyModel to clarify module ownership        |
| `PolicyModel::load_default`                  | `execution_policy_model_load_default`                            | make method purpose explicit and tie to owning type        |
| `PolicyModel::load`                          | `execution_policy_model_load`                                    | make method purpose explicit and tie to owning type        |
| `PolicyModel::save`                          | `execution_policy_model_save`                                    | make method purpose explicit and tie to owning type        |
| `PolicyModel::predict`                       | `execution_policy_model_predict`                                 | make method purpose explicit and tie to owning type        |
| `PolicyModel::decide`                        | `execution_policy_model_decide`                                  | make method purpose explicit and tie to owning type        |
| `PolicyModel::weight_norm`                   | `execution_policy_model_weight_norm`                             | make method purpose explicit and tie to owning type        |
| `default_weights`                            | `policy_model_default_weights`                                   | prefix with policy_model to reduce ambiguity               |
| `format_bias`                                | `policy_model_format_bias`                                       | prefix with policy_model to reduce ambiguity               |
| `smooth_bias`                                | `policy_model_smooth_bias`                                       | prefix with policy_model to reduce ambiguity               |
| `maybe_explore`                              | `policy_model_maybe_explore`                                     | prefix with policy_model to reduce ambiguity               |
| `EdgeSpec`                                   | `PlannerUpdateEdgeSpec`                                          | prefix with PlannerUpdate to clarify module ownership      |
| `RetractSpec`                                | `PlannerUpdateRetractSpec`                                       | prefix with PlannerUpdate to clarify module ownership      |
| `RewriteSpec`                                | `PlannerUpdateRewriteSpec`                                       | prefix with PlannerUpdate to clarify module ownership      |
| `PlannerUpdate`                              | `GraphPatch`                                                     | prefix with PlannerUpdate to clarify module ownership      |
| `apply_planner_update`                       | `apply_graph_patch`                                              | prefix with planner_update to reduce ambiguity             |
| `PlannerPhase`                               | `PlannerStage`                                                   | prefix with PlannerState to clarify module ownership       |
| `PlannerEvent`                               | `PlannerTransition`                                              | prefix with PlannerState to clarify module ownership       |
| `EndpointCtx`                                | `EndpointSelection`                                              | prefix with EndpointSelector to clarify module ownership   |
| `role_burst`                                 | `role_burst_limit`                                               | prefix with endpoint_selector to reduce ambiguity          |
| `select_endpoints_for_role`                  | `endpoint_selector_select_endpoints_for_role`                    | prefix with endpoint_selector to reduce ambiguity          |
| `StateSnapshot`                              | `PipelineSnapshot`                                               | prefix with SnapshotStore to clarify module ownership      |
| `save`                                       | `snapshot_store_save`                                            | prefix with snapshot_store to reduce ambiguity             |
| `load`                                       | `snapshot_store_load`                                            | prefix with snapshot_store to reduce ambiguity             |
| `DeltaOutcome`                               | `ExecutionDeltaOutcome`                                          | prefix with DeltaApply to clarify module ownership         |
| `DeltaRepairLog`                             | `DeltaRepairAttempt`                                             | prefix with DeltaApply to clarify module ownership         |
| `apply_read_only`                            | `apply_read_deltas`                                              | prefix with delta_apply to reduce ambiguity                |
| `summarize_deltas`                           | `summarize_execution_deltas`                                     | prefix with delta_apply to reduce ambiguity                |
| `apply_mutations`                            | `apply_write_deltas`                                             | prefix with delta_apply to reduce ambiguity                |
| `repair_delta_path`                          | `repair_delta_pathing`                                           | prefix with delta_apply to reduce ambiguity                |
| `delta_label`                                | `format_delta_label`                                             | prefix with delta_apply to reduce ambiguity                |
| `resolve_path`                               | `resolve_delta_path`                                             | prefix with delta_apply to reduce ambiguity                |
| `anchor`                                     | `anchor_path`                                                    | prefix with delta_apply to reduce ambiguity                |
| `is_within_roots`                            | `delta_apply_is_within_roots`                                    | prefix with delta_apply to reduce ambiguity                |
| `has_parent_dir_component`                   | `delta_apply_has_parent_dir_component`                           | prefix with delta_apply to reduce ambiguity                |
| `truncate_lines`                             | `delta_apply_truncate_lines`                                     | prefix with delta_apply to reduce ambiguity                |
| `GpuScheduler`                               | `GpuSchedulerGpuScheduler`                                       | prefix with GpuScheduler to clarify module ownership       |
| `GpuScheduler::schedule`                     | `gpu_scheduler_gpu_scheduler_schedule`                           | make method purpose explicit and tie to owning type        |
| `GpuScheduler::detect_deadlock`              | `gpu_scheduler_gpu_scheduler_detect_deadlock`                    | make method purpose explicit and tie to owning type        |
| `PipelineState`                              | `SchedulerState`                                                 | prefix with ExecutionScheduler to clarify module ownership |
| `PipelineEvent`                              | `SchedulerEvent`                                                 | prefix with ExecutionScheduler to clarify module ownership |
| `ExecFailure`                                | `ExecutionSchedulerExecFailure`                                  | prefix with ExecutionScheduler to clarify module ownership |
| `execute_graph_loop`                         | `run_execution_loop`                                             | prefix with execution_scheduler to reduce ambiguity        |
| `run_planner_execution_loop`                 | `run_planner_loop`                                               | prefix with execution_scheduler to reduce ambiguity        |
| `NodeOutcome`                                | `ModuleNodeOutcome`                                              | prefix with Module to clarify module ownership             |
| `ExecNodeResult`                             | `ModuleExecNodeResult`                                           | prefix with Module to clarify module ownership             |
| `ExecOutput`                                 | `ModuleExecOutput`                                               | prefix with Module to clarify module ownership             |
| `VerifyUpdate`                               | `ModuleVerifyUpdate`                                             | prefix with Module to clarify module ownership             |
| `VerifyOutput`                               | `ModuleVerifyOutput`                                             | prefix with Module to clarify module ownership             |
| `NodeCallResult`                             | `ModuleNodeCallResult`                                           | prefix with Module to clarify module ownership             |
| `call_llm_raw_with_retry_allow_mismatch`     | `module_call_llm_raw_with_retry_allow_mismatch`                  | prefix with module to reduce ambiguity                     |
| `call_llm_json_with_retry_allow_mismatch`    | `module_call_llm_json_with_retry_allow_mismatch`                 | prefix with module to reduce ambiguity                     |
| `init_io_workers`                            | `module_init_io_workers`                                         | prefix with module to reduce ambiguity                     |
| `take_recovery_signal`                       | `module_take_recovery_signal`                                    | prefix with module to reduce ambiguity                     |
| `NodeProcessReport`                          | `ModuleNodeProcessReport`                                        | prefix with Module to clarify module ownership             |
| `DispatchMode`                               | `ModuleDispatchMode`                                             | prefix with Module to clarify module ownership             |
| `ModeConfig`                                 | `ModuleModeConfig`                                               | prefix with Module to clarify module ownership             |
| `log_name_mutate`                            | `module_log_name_mutate`                                         | prefix with module to reduce ambiguity                     |
| `log_name_verify`                            | `module_log_name_verify`                                         | prefix with module to reduce ambiguity                     |
| `log_name_readonly`                          | `module_log_name_readonly`                                       | prefix with module to reduce ambiguity                     |
| `ParseFn`                                    | `ModuleParseFn`                                                  | prefix with Module to clarify module ownership             |
| `repair_node`                                | `module_repair_node`                                             | prefix with module to reduce ambiguity                     |
| `process_call_result`                        | `module_process_call_result`                                     | prefix with module to reduce ambiguity                     |
| `rule_retry`                                 | `module_rule_retry`                                              | prefix with module to reduce ambiguity                     |
| `rule_capability_downgrade`                  | `module_rule_capability_downgrade`                               | prefix with module to reduce ambiguity                     |
| `rule_dependency_rewire`                     | `module_rule_dependency_rewire`                                  | prefix with module to reduce ambiguity                     |
| `rule_node_split`                            | `module_rule_node_split`                                         | prefix with module to reduce ambiguity                     |
| `parse_mutate`                               | `module_parse_mutate`                                            | prefix with module to reduce ambiguity                     |
| `parse_verify`                               | `module_parse_verify`                                            | prefix with module to reduce ambiguity                     |
| `parse_readonly`                             | `module_parse_readonly`                                          | prefix with module to reduce ambiguity                     |
| `ModePredicate`                              | `ModuleModePredicate`                                            | prefix with Module to clarify module ownership             |
| `ModeValidator`                              | `ModuleModeValidator`                                            | prefix with Module to clarify module ownership             |
| `ModeRule`                                   | `ModuleModeRule`                                                 | prefix with Module to clarify module ownership             |
| `validate_verify`                            | `module_validate_verify`                                         | prefix with module to reduce ambiguity                     |
| `validate_mutate`                            | `module_validate_mutate`                                         | prefix with module to reduce ambiguity                     |
| `validate_pass`                              | `module_validate_pass`                                           | prefix with module to reduce ambiguity                     |
| `select_mode`                                | `module_select_mode`                                             | prefix with module to reduce ambiguity                     |
| `call_node`                                  | `module_call_node`                                               | prefix with module to reduce ambiguity                     |
| `apply_node_result`                          | `module_apply_node_result`                                       | prefix with module to reduce ambiguity                     |
| `dispatch_node`                              | `module_dispatch_node`                                           | prefix with module to reduce ambiguity                     |
| `call_mode`                                  | `module_call_mode`                                               | prefix with module to reduce ambiguity                     |
| `llm_call_with_retry`                        | `module_llm_call_with_retry`                                     | prefix with module to reduce ambiguity                     |
| `apply_mutate_output`                        | `module_apply_mutate_output`                                     | prefix with module to reduce ambiguity                     |
| `apply_verify_output`                        | `module_apply_verify_output`                                     | prefix with module to reduce ambiguity                     |
| `apply_readonly_output`                      | `module_apply_readonly_output`                                   | prefix with module to reduce ambiguity                     |
| `partition_deltas`                           | `module_partition_deltas`                                        | prefix with module to reduce ambiguity                     |
| `mutate_is_blocked`                          | `module_mutate_is_blocked`                                       | prefix with module to reduce ambiguity                     |
| `apply_mutate_result`                        | `module_apply_mutate_result`                                     | prefix with module to reduce ambiguity                     |
| `log_empty_readonly`                         | `module_log_empty_readonly`                                      | prefix with module to reduce ambiguity                     |
| `log_readonly_error`                         | `module_log_readonly_error`                                      | prefix with module to reduce ambiguity                     |
| `apply_readonly_result`                      | `module_apply_readonly_result`                                   | prefix with module to reduce ambiguity                     |
| `coerce_id`                                  | `module_coerce_id`                                               | prefix with module to reduce ambiguity                     |
| `parse_exec_output`                          | `module_parse_exec_output`                                       | prefix with module to reduce ambiguity                     |
| `ExecStep`                                   | `ExecutionStep`                                                  | prefix with ExecutionState to clarify module ownership     |
| `ExecEvent`                                  | `ExecutionEvent`                                                 | prefix with ExecutionState to clarify module ownership     |
| `TemplateStore`                              | `GraphTemplateStore`                                             | prefix with TemplateStore to clarify module ownership      |
| `TemplateStore::new`                         | `graph_template_store_new`                                       | make method purpose explicit and tie to owning type        |
| `TemplateStore::path_for`                    | `graph_template_store_path_for`                                  | make method purpose explicit and tie to owning type        |
| `TemplateStore::hash_for`                    | `graph_template_store_hash_for`                                  | make method purpose explicit and tie to owning type        |
| `TemplateStore::reward_path`                 | `graph_template_store_reward_path`                               | make method purpose explicit and tie to owning type        |
| `TemplateStore::history_path`                | `graph_template_store_history_path`                              | make method purpose explicit and tie to owning type        |
| `TemplateStore::load`                        | `graph_template_store_load`                                      | make method purpose explicit and tie to owning type        |
| `TemplateStore::save`                        | `graph_template_store_save`                                      | make method purpose explicit and tie to owning type        |
| `TemplateStore::stored_reward`               | `graph_template_store_stored_reward`                             | make method purpose explicit and tie to owning type        |
| `TemplateStore::record_reward`               | `graph_template_store_record_reward`                             | make method purpose explicit and tie to owning type        |
| `TemplateStore::recent_rewards`              | `graph_template_store_recent_rewards`                            | make method purpose explicit and tie to owning type        |
| `TemplateStore::is_plateaued`                | `graph_template_store_is_plateaued`                              | make method purpose explicit and tie to owning type        |
| `TemplateStore::save_with_reward`            | `graph_template_store_save_with_reward`                          | make method purpose explicit and tie to owning type        |
| `TemplateStore::update`                      | `graph_template_store_update`                                    | make method purpose explicit and tie to owning type        |
| `TemplateStore::exists`                      | `graph_template_store_exists`                                    | make method purpose explicit and tie to owning type        |
| `TemplateStore::evict`                       | `graph_template_store_evict`                                     | make method purpose explicit and tie to owning type        |
| `TemplateStore::find_similar`                | `graph_template_store_find_similar`                              | make method purpose explicit and tie to owning type        |
| `TemplateStore::record_failure`              | `graph_template_store_record_failure`                            | make method purpose explicit and tie to owning type        |
| `TemplateStore::record_revision`             | `graph_template_store_record_revision`                           | make method purpose explicit and tie to owning type        |
| `tag`                                        | `console_ui_tag`                                                 | prefix with console_ui to reduce ambiguity                 |
| `info`                                       | `console_ui_info`                                                | prefix with console_ui to reduce ambiguity                 |
| `warn`                                       | `console_ui_warn`                                                | prefix with console_ui to reduce ambiguity                 |
| `err`                                        | `console_ui_err`                                                 | prefix with console_ui to reduce ambiguity                 |
| `phase`                                      | `console_ui_phase`                                               | prefix with console_ui to reduce ambiguity                 |
| `llm`                                        | `console_ui_llm`                                                 | prefix with console_ui to reduce ambiguity                 |
| `mode_label`                                 | `console_ui_mode_label`                                          | prefix with console_ui to reduce ambiguity                 |
| `mode_tag`                                   | `console_ui_mode_tag`                                            | prefix with console_ui to reduce ambiguity                 |
| `dim`                                        | `console_ui_dim`                                                 | prefix with console_ui to reduce ambiguity                 |
| `truncate`                                   | `console_ui_truncate`                                            | prefix with console_ui to reduce ambiguity                 |
| `DispatchCtx`                                | `NodeDispatchContext`                                            | prefix with NodeDispatch to clarify module ownership       |
| `resolve_endpoint`                           | `node_dispatch_resolve_endpoint`                                 | prefix with node_dispatch to reduce ambiguity              |
| `log_dispatch`                               | `node_dispatch_log_dispatch`                                     | prefix with node_dispatch to reduce ambiguity              |
| `dispatch_node_call`                         | `dispatch_node_execution`                                        | prefix with node_dispatch to reduce ambiguity              |
| `compute_ready`                              | `graph_cpu_kernels_compute_ready`                                | prefix with graph_cpu_kernels to reduce ambiguity          |
| `priority_sort`                              | `graph_cpu_kernels_priority_sort`                                | prefix with graph_cpu_kernels to reduce ambiguity          |
| `deadlock_check`                             | `graph_cpu_kernels_deadlock_check`                               | prefix with graph_cpu_kernels to reduce ambiguity          |
| `compute_topo_order`                         | `graph_cpu_kernels_compute_topo_order`                           | prefix with graph_cpu_kernels to reduce ambiguity          |
| `compute_roots`                              | `graph_cpu_kernels_compute_roots`                                | prefix with graph_cpu_kernels to reduce ambiguity          |
| `compute_scc`                                | `graph_cpu_kernels_compute_scc`                                  | prefix with graph_cpu_kernels to reduce ambiguity          |
| `compute_reachability`                       | `graph_cpu_kernels_compute_reachability`                         | prefix with graph_cpu_kernels to reduce ambiguity          |
| `compute_depth`                              | `graph_cpu_kernels_compute_depth`                                | prefix with graph_cpu_kernels to reduce ambiguity          |
| `RawConfig`                                  | `CapabilityConfigRawConfig`                                      | prefix with CapabilityConfig to clarify module ownership   |
| `RawSystem`                                  | `CapabilityConfigRawSystem`                                      | prefix with CapabilityConfig to clarify module ownership   |
| `RawLlm`                                     | `CapabilityConfigRawLlm`                                         | prefix with CapabilityConfig to clarify module ownership   |
| `RawEndpoints`                               | `CapabilityConfigRawEndpoints`                                   | prefix with CapabilityConfig to clarify module ownership   |
| `Default::default`                           | `capability_config_default_default`                              | make method purpose explicit and tie to owning type        |
| `default_max_output_lines`                   | `capability_config_default_max_output_lines`                     | prefix with capability_config to reduce ambiguity          |
| `default_max_iterations`                     | `capability_config_default_max_iterations`                       | prefix with capability_config to reduce ambiguity          |
| `default_llm_retry_count`                    | `capability_config_default_llm_retry_count`                      | prefix with capability_config to reduce ambiguity          |
| `default_llm_retry_delay`                    | `capability_config_default_llm_retry_delay`                      | prefix with capability_config to reduce ambiguity          |
| `default_response_timeout_secs`              | `capability_config_default_response_timeout_secs`                | prefix with capability_config to reduce ambiguity          |
| `default_max_concurrency`                    | `capability_config_default_max_concurrency`                      | prefix with capability_config to reduce ambiguity          |
| `default_max_nodes`                          | `capability_config_default_max_nodes`                            | prefix with capability_config to reduce ambiguity          |
| `default_max_expand_iters`                   | `capability_config_default_max_expand_iters`                     | prefix with capability_config to reduce ambiguity          |
| `default_context_radius`                     | `capability_config_default_context_radius`                       | prefix with capability_config to reduce ambiguity          |
| `default_max_depth`                          | `capability_config_default_max_depth`                            | prefix with capability_config to reduce ambiguity          |
| `default_prune_unlinked`                     | `capability_config_default_prune_unlinked`                       | prefix with capability_config to reduce ambiguity          |
| `default_planner_max_new_nodes`              | `capability_config_default_planner_max_new_nodes`                | prefix with capability_config to reduce ambiguity          |
| `default_planner_max_new_edges`              | `capability_config_default_planner_max_new_edges`                | prefix with capability_config to reduce ambiguity          |
| `default_planner_refine_on_cache`            | `capability_config_default_planner_refine_on_cache`              | prefix with capability_config to reduce ambiguity          |
| `default_planner_plateau_window`             | `capability_config_default_planner_plateau_window`               | prefix with capability_config to reduce ambiguity          |
| `default_planner_plateau_threshold`          | `capability_config_default_planner_plateau_threshold`            | prefix with capability_config to reduce ambiguity          |
| `default_planner_plateau_expand_factor`      | `capability_config_default_planner_plateau_expand_factor`        | prefix with capability_config to reduce ambiguity          |
| `default_auto_prune`                         | `capability_config_default_auto_prune`                           | prefix with capability_config to reduce ambiguity          |
| `default_prune_threshold`                    | `capability_config_default_prune_threshold`                      | prefix with capability_config to reduce ambiguity          |
| `default_prune_min_age`                      | `capability_config_default_prune_min_age`                        | prefix with capability_config to reduce ambiguity          |
| `default_template_reuse_threshold`           | `capability_config_default_template_reuse_threshold`             | prefix with capability_config to reduce ambiguity          |
| `default_template_top_k`                     | `capability_config_default_template_top_k`                       | prefix with capability_config to reduce ambiguity          |
| `default_recovery_retry_rate_threshold`      | `capability_config_default_recovery_retry_rate_threshold`        | prefix with capability_config to reduce ambiguity          |
| `default_recovery_failed_fraction_threshold` | `capability_config_default_recovery_failed_fraction_threshold`   | prefix with capability_config to reduce ambiguity          |
| `default_max_node_retries`                   | `capability_config_default_max_node_retries`                     | prefix with capability_config to reduce ambiguity          |
| `default_repair_radius`                      | `capability_config_default_repair_radius`                        | prefix with capability_config to reduce ambiguity          |
| `default_max_repairs_per_node`               | `capability_config_default_max_repairs_per_node`                 | prefix with capability_config to reduce ambiguity          |
| `default_cost_latency_weight`                | `capability_config_default_cost_latency_weight`                  | prefix with capability_config to reduce ambiguity          |
| `default_cost_failure_weight`                | `capability_config_default_cost_failure_weight`                  | prefix with capability_config to reduce ambiguity          |
| `default_cost_decay_rate`                    | `capability_config_default_cost_decay_rate`                      | prefix with capability_config to reduce ambiguity          |
| `default_mutation_rate`                      | `capability_config_default_mutation_rate`                        | prefix with capability_config to reduce ambiguity          |
| `default_mutation_budget`                    | `capability_config_default_mutation_budget`                      | prefix with capability_config to reduce ambiguity          |
| `default_mutation_candidates`                | `capability_config_default_mutation_candidates`                  | prefix with capability_config to reduce ambiguity          |
| `default_template_population_size`           | `capability_config_default_template_population_size`             | prefix with capability_config to reduce ambiguity          |
| `default_failure_constraint_threshold`       | `capability_config_default_failure_constraint_threshold`         | prefix with capability_config to reduce ambiguity          |
| `default_max_constraints`                    | `capability_config_default_max_constraints`                      | prefix with capability_config to reduce ambiguity          |
| `default_enable_resume`                      | `capability_config_default_enable_resume`                        | prefix with capability_config to reduce ambiguity          |
| `default_snapshot_interval_iters`            | `capability_config_default_snapshot_interval_iters`              | prefix with capability_config to reduce ambiguity          |
| `default_snapshot_file`                      | `capability_config_default_snapshot_file`                        | prefix with capability_config to reduce ambiguity          |
| `default_goal_similarity_weight`             | `capability_config_default_goal_similarity_weight`               | prefix with capability_config to reduce ambiguity          |
| `default_structural_similarity_weight`       | `capability_config_default_structural_similarity_weight`         | prefix with capability_config to reduce ambiguity          |
| `default_embedding_model`                    | `capability_config_default_embedding_model`                      | prefix with capability_config to reduce ambiguity          |
| `default_embedding_dim`                      | `capability_config_default_embedding_dim`                        | prefix with capability_config to reduce ambiguity          |
| `default_max_tabs`                           | `capability_config_default_max_tabs`                             | prefix with capability_config to reduce ambiguity          |
| `default_tab_cooldown_ms`                    | `capability_config_default_tab_cooldown_ms`                      | prefix with capability_config to reduce ambiguity          |
| `RawRoleConfig`                              | `CapabilityConfigRawRoleConfig`                                  | prefix with CapabilityConfig to clarify module ownership   |
| `LlmEndpoint`                                | `CapabilityConfigLlmEndpoint`                                    | prefix with CapabilityConfig to clarify module ownership   |
| `CapabilityConfig`                           | `CapabilityConfigCapabilityConfig`                               | prefix with CapabilityConfig to clarify module ownership   |
| `CapabilityConfig::load`                     | `capability_config_capability_config_load`                       | make method purpose explicit and tie to owning type        |
| `CapabilityConfig::endpoint_by_id`           | `capability_config_capability_config_endpoint_by_id`             | make method purpose explicit and tie to owning type        |
| `CapabilityConfig::role_config`              | `capability_config_capability_config_role_config`                | make method purpose explicit and tie to owning type        |
| `CapabilityConfig::planner_endpoint`         | `capability_config_capability_config_planner_endpoint`           | make method purpose explicit and tie to owning type        |
| `GoalSpec`                                   | `CapabilityConfigGoalSpec`                                       | prefix with CapabilityConfig to clarify module ownership   |
| `GoalSpec::new`                              | `capability_config_goal_spec_new`                                | make method purpose explicit and tie to owning type        |
| `GoalSpec::from_file`                        | `capability_config_goal_spec_from_file`                          | make method purpose explicit and tie to owning type        |
| `RawPolicy`                                  | `CapabilityConfigRawPolicy`                                      | prefix with CapabilityConfig to clarify module ownership   |
| `CapabilityPolicy`                           | `CapabilityConfigCapabilityPolicy`                               | prefix with CapabilityConfig to clarify module ownership   |
| `CapabilityPolicy::load`                     | `capability_config_capability_policy_load`                       | make method purpose explicit and tie to owning type        |
| `PolicyOutcome`                              | `PolicyEvalPolicyOutcome`                                        | prefix with PolicyEval to clarify module ownership         |
| `evaluate`                                   | `evaluate_policy`                                                | prefix with policy_eval to reduce ambiguity                |
| `score_ready_nodes`                          | `scheduler_scoring_score_ready_nodes`                            | prefix with scheduler_scoring to reduce ambiguity          |
| `score_node`                                 | `scheduler_scoring_score_node`                                   | prefix with scheduler_scoring to reduce ambiguity          |
| `ReadHandler`                                | `DeltaExecutorReadHandler`                                       | prefix with DeltaExecutor to clarify module ownership      |
| `WriteHandler`                               | `DeltaExecutorWriteHandler`                                      | prefix with DeltaExecutor to clarify module ownership      |
| `DeltaType`                                  | `ExecutionDeltaType`                                             | prefix with DeltaExecutor to clarify module ownership      |
| `execute_read_only`                          | `execute_read_delta`                                             | prefix with delta_executor to reduce ambiguity             |
| `execute_mutation`                           | `execute_write_delta`                                            | prefix with delta_executor to reduce ambiguity             |
| `delta_type`                                 | `delta_executor_delta_type`                                      | prefix with delta_executor to reduce ambiguity             |
| `handle_read_file`                           | `apply_read_file`                                                | prefix with delta_executor to reduce ambiguity             |
| `handle_list_dir`                            | `apply_list_dir`                                                 | prefix with delta_executor to reduce ambiguity             |
| `handle_read_command`                        | `apply_read_command`                                             | prefix with delta_executor to reduce ambiguity             |
| `handle_write_file`                          | `apply_write_file`                                               | prefix with delta_executor to reduce ambiguity             |
| `handle_replace_text`                        | `apply_replace_text`                                             | prefix with delta_executor to reduce ambiguity             |
| `handle_delete_file`                         | `apply_delete_file`                                              | prefix with delta_executor to reduce ambiguity             |
| `CapabilityCost`                             | `CapabilityCostCapabilityCost`                                   | prefix with CapabilityCost to clarify module ownership     |
| `CapabilityCostTable`                        | `CapabilityCostCapabilityCostTable`                              | prefix with CapabilityCost to clarify module ownership     |
| `CapabilityCostTable::load`                  | `capability_cost_capability_cost_table_load`                     | make method purpose explicit and tie to owning type        |
| `CapabilityCostTable::save`                  | `capability_cost_capability_cost_table_save`                     | make method purpose explicit and tie to owning type        |
| `CapabilityCostTable::update`                | `capability_cost_capability_cost_table_update`                   | make method purpose explicit and tie to owning type        |
| `CapabilityCostTable::node_cost`             | `capability_cost_capability_cost_table_node_cost`                | make method purpose explicit and tie to owning type        |
| `CapabilityCostTable::avg_latency`           | `capability_cost_capability_cost_table_avg_latency`              | make method purpose explicit and tie to owning type        |
| `CapabilityCostTable::avg_failure`           | `capability_cost_capability_cost_table_avg_failure`              | make method purpose explicit and tie to owning type        |
| `CapabilityCostTable::summary`               | `capability_cost_capability_cost_table_summary`                  | make method purpose explicit and tie to owning type        |
| `apply_node_cost_update`                     | `capability_cost_apply_node_cost_update`                         | prefix with capability_cost to reduce ambiguity            |
| `PolicyDatasetEntry`                         | `PolicyTrainingPolicyDatasetEntry`                               | prefix with PolicyTraining to clarify module ownership     |
| `load_dataset`                               | `policy_training_load_dataset`                                   | prefix with policy_training to reduce ambiguity            |
| `dataset_size`                               | `policy_training_dataset_size`                                   | prefix with policy_training to reduce ambiguity            |
| `features_from_json`                         | `policy_training_features_from_json`                             | prefix with policy_training to reduce ambiguity            |
| `train_policy`                               | `train_policy_weights`                                           | prefix with policy_training to reduce ambiguity            |
| `save_weights`                               | `policy_training_save_weights`                                   | prefix with policy_training to reduce ambiguity            |
| `update_online`                              | `policy_training_update_online`                                  | prefix with policy_training to reduce ambiguity            |
| `append_policy_dataset`                      | `policy_training_append_policy_dataset`                          | prefix with policy_training to reduce ambiguity            |
| `dot`                                        | `policy_training_dot`                                            | prefix with policy_training to reduce ambiguity            |
| `ensure_head_len`                            | `policy_training_ensure_head_len`                                | prefix with policy_training to reduce ambiguity            |
| `update_head`                                | `policy_training_update_head`                                    | prefix with policy_training to reduce ambiguity            |
| `register`                                   | `response_router_register`                                       | prefix with response_router to reduce ambiguity            |
| `resolve`                                    | `response_router_resolve`                                        | prefix with response_router to reduce ambiguity            |
| `new_tabs`                                   | `llm_worker_new_tabs`                                            | prefix with llm_worker to reduce ambiguity                 |
| `LlmRequest`                                 | `LlmWorkItem`                                                    | prefix with LlmWorker to clarify module ownership          |
| `EndpointWorker`                             | `LlmWorker`                                                      | prefix with LlmWorker to clarify module ownership          |
| `EndpointWorker::handle_request`             | `llm_worker_handle_request`                                      | make method purpose explicit and tie to owning type        |
| `EndpointWorker::send_turn`                  | `llm_worker_send_turn`                                           | make method purpose explicit and tie to owning type        |
| `send_request`                               | `llm_worker_send_request`                                        | prefix with llm_worker to reduce ambiguity                 |
| `init_workers`                               | `llm_worker_init_workers`                                        | prefix with llm_worker to reduce ambiguity                 |
| `run_worker`                                 | `llm_worker_run_worker`                                          | prefix with llm_worker to reduce ambiguity                 |
| `stable_hash64`                              | `llm_worker_stable_hash64`                                       | prefix with llm_worker to reduce ambiguity                 |
| `response_matches_req_id`                    | `llm_worker_response_matches_req_id`                             | prefix with llm_worker to reduce ambiguity                 |
| `MaintenanceCtx`                             | `GraphRepairMaintenanceCtx`                                      | prefix with GraphRepair to clarify module ownership        |
| `prune_low_value_nodes`                      | `prune_low_utility_nodes`                                        | prefix with graph_repair to reduce ambiguity               |
| `apply_recovery`                             | `recover_from_failures`                                          | prefix with graph_repair to reduce ambiguity               |
| `maintain_graph`                             | `repair_graph`                                                   | prefix with graph_repair to reduce ambiguity               |
| `risk_score`                                 | `graph_repair_risk_score`                                        | prefix with graph_repair to reduce ambiguity               |
| `FailureEntry`                               | `FailureStoreFailureEntry`                                       | prefix with FailureStore to clarify module ownership       |
| `FailureLogEntry`                            | `FailureStoreFailureLogEntry`                                    | prefix with FailureStore to clarify module ownership       |
| `FailureFile`                                | `FailureStoreFailureFile`                                        | prefix with FailureStore to clarify module ownership       |
| `FailureStore`                               | `FailureStoreFailureStore`                                       | prefix with FailureStore to clarify module ownership       |
| `FailureStats`                               | `FailureStoreFailureStats`                                       | prefix with FailureStore to clarify module ownership       |
| `Constraint`                                 | `FailureStoreConstraint`                                         | prefix with FailureStore to clarify module ownership       |
| `ConstraintRule`                             | `FailureStoreConstraintRule`                                     | prefix with FailureStore to clarify module ownership       |
| `FailureStore::load`                         | `failure_store_failure_store_load`                               | make method purpose explicit and tie to owning type        |
| `FailureStore::contains`                     | `failure_store_failure_store_contains`                           | make method purpose explicit and tie to owning type        |
| `FailureStore::failure_count`                | `failure_store_failure_store_failure_count`                      | make method purpose explicit and tie to owning type        |
| `FailureStore::stats`                        | `failure_store_failure_store_stats`                              | make method purpose explicit and tie to owning type        |
| `FailureStore::constraints`                  | `failure_store_failure_store_constraints`                        | make method purpose explicit and tie to owning type        |
| `FailureStore::record`                       | `failure_store_failure_store_record`                             | make method purpose explicit and tie to owning type        |
| `FailureStore::record_graph`                 | `failure_store_failure_store_record_graph`                       | make method purpose explicit and tie to owning type        |
| `FailureStore::persist`                      | `failure_store_failure_store_persist`                            | make method purpose explicit and tie to owning type        |
| `FailureStore::append_log`                   | `failure_store_failure_store_append_log`                         | make method purpose explicit and tie to owning type        |
| `PlannerMetrics`                             | `PlannerTelemetry`                                               | prefix with Telemetry to clarify module ownership          |
| `ExecMetrics`                                | `ExecutionTelemetry`                                             | prefix with Telemetry to clarify module ownership          |
| `RuntimeMetrics`                             | `RuntimeTelemetry`                                               | prefix with Telemetry to clarify module ownership          |
| `TelemetrySnapshot`                          | `TelemetryFrame`                                                 | prefix with Telemetry to clarify module ownership          |
| `record_snapshot`                            | `telemetry_record_snapshot`                                      | prefix with telemetry to reduce ambiguity                  |
| `record_all_snapshots`                       | `telemetry_record_all_snapshots`                                 | prefix with telemetry to reduce ambiguity                  |
| `update_avg_u64`                             | `telemetry_update_avg_u64`                                       | prefix with telemetry to reduce ambiguity                  |
| `progress_fraction`                          | `telemetry_progress_fraction`                                    | prefix with telemetry to reduce ambiguity                  |
| `compute_reward`                             | `telemetry_compute_reward`                                       | prefix with telemetry to reduce ambiguity                  |
| `pending_requests`                           | `telemetry_pending_requests`                                     | prefix with telemetry to reduce ambiguity                  |
| `set_resume_iteration`                       | `telemetry_set_resume_iteration`                                 | prefix with telemetry to reduce ambiguity                  |
| `resume_iteration`                           | `telemetry_resume_iteration`                                     | prefix with telemetry to reduce ambiguity                  |
| `inc_pending`                                | `telemetry_inc_pending`                                          | prefix with telemetry to reduce ambiguity                  |
| `dec_pending`                                | `telemetry_dec_pending`                                          | prefix with telemetry to reduce ambiguity                  |
| `TabSlots`                                   | `TabSlotTable`                                                   | prefix with TabManager to clarify module ownership         |
| `TabSlots::new`                              | `tab_slot_table_new`                                             | make method purpose explicit and tie to owning type        |
| `TabMeta`                                    | `TabSlotMeta`                                                    | prefix with TabManager to clarify module ownership         |
| `TabsHandle`                                 | `TabManagerHandle`                                               | prefix with TabManager to clarify module ownership         |
| `get_or_open_tab`                            | `tab_manager_get_or_open_tab`                                    | prefix with tab_manager to reduce ambiguity                |
| `get_owner_tab`                              | `tab_manager_get_owner_tab`                                      | prefix with tab_manager to reduce ambiguity                |
| `set_tab_id`                                 | `tab_manager_set_tab_id`                                         | prefix with tab_manager to reduce ambiguity                |
| `mark_tab_sent`                              | `tab_manager_mark_tab_sent`                                      | prefix with tab_manager to reduce ambiguity                |
| `mark_tab_response`                          | `tab_manager_mark_tab_response`                                  | prefix with tab_manager to reduce ambiguity                |
| `mark_tab_in_flight`                         | `tab_manager_mark_tab_in_flight`                                 | prefix with tab_manager to reduce ambiguity                |
| `mark_tab_cooldown`                          | `tab_manager_mark_tab_cooldown`                                  | prefix with tab_manager to reduce ambiguity                |
| `drop_tab`                                   | `tab_manager_drop_tab`                                           | prefix with tab_manager to reduce ambiguity                |
| `summarize_tab_state`                        | `tab_manager_summarize_tab_state`                                | prefix with tab_manager to reduce ambiguity                |
| `now_ms`                                     | `tab_manager_now_ms`                                             | prefix with tab_manager to reduce ambiguity                |
| `log_llm`                                    | `tab_manager_log_llm`                                            | prefix with tab_manager to reduce ambiguity                |
| `GoalEmbedding`                              | `GoalEmbeddingGoalEmbedding`                                     | prefix with GoalEmbedding to clarify module ownership      |
| `embed_goal`                                 | `goal_embedding_embed_goal`                                      | prefix with goal_embedding to reduce ambiguity             |
| `cosine_similarity`                          | `goal_embedding_cosine_similarity`                               | prefix with goal_embedding to reduce ambiguity             |
| `load_cache`                                 | `goal_embedding_load_cache`                                      | prefix with goal_embedding to reduce ambiguity             |
| `save_cache`                                 | `goal_embedding_save_cache`                                      | prefix with goal_embedding to reduce ambiguity             |
| `goal_hash`                                  | `goal_embedding_goal_hash`                                       | prefix with goal_embedding to reduce ambiguity             |
| `fnv64`                                      | `goal_embedding_fnv64`                                           | prefix with goal_embedding to reduce ambiguity             |


## File Renames
| Old Name | New Name | Reason |
| --- | --- | --- |
| `capability_capability.rs` | `Types_capability.rs` | domain prefix |
| `capability_capability_cost.rs` | `Types_capability_cost.rs` | domain prefix |
| `capability_config.rs` | `Types_config.rs` | domain prefix |
| `capability_console.rs` | `IO_console.rs` | domain prefix |
| `capability_dag.rs` | `Graph_dag.rs` | domain prefix |
| `capability_dispatch.rs` | `Execution_dispatch.rs` | domain prefix |
| `capability_endpoint_scheduler.rs` | `Execution_endpoint_scheduler.rs` | domain prefix |
| `capability_endpoint_worker.rs` | `Execution_endpoint_worker.rs` | domain prefix |
| `capability_engine.rs` | `Engine_engine.rs` | domain prefix |
| `capability_execution_result.rs` | `Execution_result.rs` | domain prefix |
| `capability_executor_dispatch.rs` | `Execution_executor_dispatch.rs` | domain prefix |
| `capability_failure_store.rs` | `Types_failure_store.rs` | domain prefix |
| `capability_goal_embedding.rs` | `Types_goal_embedding.rs` | domain prefix |
| `capability_gpu_scheduler.rs` | `Engine_gpu_scheduler.rs` | domain prefix |
| `capability_gpu_scheduler_driver.rs` | `Engine_gpu_scheduler_driver.rs` | domain prefix |
| `capability_gpu_scheduler_kernels.rs` | `Engine_gpu_scheduler_kernels.rs` | domain prefix |
| `capability_gpu_scheduler_layout.rs` | `Engine_gpu_scheduler_layout.rs` | domain prefix |
| `capability_graph_algo.rs` | `Graph_algo.rs` | domain prefix |
| `capability_graph_maintenance.rs` | `Graph_maintenance.rs` | domain prefix |
| `capability_graph_runtime.rs` | `Graph_runtime.rs` | domain prefix |
| `capability_llm.rs` | `IO_llm.rs` | domain prefix |
| `capability_planner_session.rs` | `Planner_session.rs` | domain prefix |
| `capability_planner_state.rs` | `Planner_state.rs` | domain prefix |
| `capability_planner_update.rs` | `Planner_update.rs` | domain prefix |
| `capability_policy.rs` | `Policy_policy.rs` | domain prefix |
| `capability_policy_engine.rs` | `Policy_engine.rs` | domain prefix |
| `capability_policy_train.rs` | `Policy_train.rs` | domain prefix |
| `capability_response_router.rs` | `Policy_response_router.rs` | domain prefix |
| `capability_scheduler.rs` | `Execution_scheduler.rs` | domain prefix |
| `capability_scheduler_scoring.rs` | `Execution_scheduler_scoring.rs` | domain prefix |
| `capability_scheduler_state.rs` | `Execution_scheduler_state.rs` | domain prefix |
| `capability_state_snapshot.rs` | `Execution_state_snapshot.rs` | domain prefix |
| `capability_tab_management.rs` | `IO_tab_management.rs` | domain prefix |
| `capability_telemetry.rs` | `IO_telemetry.rs` | domain prefix |
| `capability_template_index.rs` | `Planner_template_index.rs` | domain prefix |
| `capability_template_mutation.rs` | `Planner_template_mutation.rs` | domain prefix |
| `ws_server.rs` | `IO_ws_server.rs` | domain prefix |

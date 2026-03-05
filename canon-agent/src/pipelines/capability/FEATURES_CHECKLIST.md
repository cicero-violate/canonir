# Capability Pipeline Feature Checklist

Checklist of intended features and the primary function/module that enables them.

| Status | Feature                                                  | Enabled by (function/module)                                                                                                 |
| ---    | ---                                                      | ---                                                                                                                          |
| [x]    | Template auto-selection (reuse vs planner)               | `scheduler::run_planner_execution_loop` (template reuse block), `TemplateStore::find_similar`, `TemplateIndex::find_similar` |
| [x]    | Policy gate for planner execution                        | `policy_engine::evaluate`, `scheduler::run_planner_execution_loop`                                                           |
| [x]    | Template load & reset for execution                      | `TemplateStore::load`, `TaskGraph::reset_for_execution`                                                                      |
| [x]    | Template reward tracking                                 | `TemplateStore::record_reward`, `TemplateStore::save_with_reward`                                                            |
| [x]    | Template failure tracking                                | `TemplateStore::record_failure`, `FailureStore::record_graph`                                                                |
| [x]    | Planner update application                               | `templates::apply_planner_update`, `run_planner_execution_loop`                                                              |
| [x]    | Graph repair operator (retry / rewrite / rewire / split) | `engine::repair_node` + rules (`rule_retry`, `rule_capability_downgrade`, `rule_dependency_rewire`, `rule_node_split`)       |
| [x]    | Repair budget enforcement                                | `TaskNode::repair_attempts`, `config.max_repairs_per_node`, `engine::repair_node`                                            |
| [x]    | Failure constraint injection                             | `FailureStore::constraints`, `planner_session::validate_planner_update`                                                      |
| [x]    | Failure signature rejection                              | `FailureStore::contains`, `graph_algo::graph_signature`, `run_planner_execution_loop`                                        |
| [x]    | Constraint rejection telemetry                           | `RuntimeMetrics.constraint_rejections`, `constraint_hit_rate`, `constraint_types`                                            |
| [x]    | Long-horizon credit assignment                           | `policy_train::update_online`, `policy_train::append_policy_dataset`                                                         |
| [x]    | Policy dataset capture                                   | `policy_train::append_policy_dataset`                                                                                        |
| [x]    | Policy decision logging                                  | `RuntimeMetrics.policy_*` fields in `telemetry.rs`                                                                           |
| [x]    | Capability cost model                                    | `capability_cost::CapabilityCostTable`, `process_node_result` updates                                                        |
| [x]    | Cost-aware scheduling                                    | `scheduler_scoring::score_ready_nodes`, `CapabilityCostTable::node_cost`                                                     |
| [x]    | Cost-aware planner hints                                 | `CapabilityCostTable::summary` passed to `PlannerSession::build_prompt`                                                      |
| [x]    | Template mutation engine                                 | `template_mutation::generate_candidates`, `template_mutation::evaluate_candidates`, `run_planner_execution_loop`             |
| [x]    | Mutation evaluation & selection                          | `TemplateStore::save_with_reward`, `template_mutation::evaluate_candidates`                                                  |
| [x]    | Deterministic resume snapshot save                       | `state_snapshot::save`, `execute_graph_loop` (snapshot interval)                                                             |
| [x]    | Deterministic resume snapshot load                       | `state_snapshot::load`, `CapabilityPipeline::run_capability_loop`                                                            |
| [x]    | Resume iteration tracking                                | `telemetry::set_resume_iteration`, `RuntimeMetrics.resume_iteration`                                                         |
| [x]    | Goal similarity embeddings                               | `goal_embedding::embed_goal`, `TemplateIndex::find_similar`                                                                  |
| [x]    | Embedding cache                                          | `goal_embedding::load_cache`, `goal_embedding::save_cache`                                                                   |
| [x]    | Similarity-weighted template retrieval                   | `TemplateIndex::find_similar` (goal + structural weights)                                                                    |
| [x]    | Graph maintenance & pruning                              | `graph_maintenance::maintain_graph`, `prune_unlinked_nodes`, `prune_low_value_nodes`                                         |
| [x]    | Deadlock detection                                       | `GpuScheduler::detect_deadlock`, `gpu_scheduler::kernels::deadlock_check`                                                    |
| [x]    | Delta auto-repair (missing paths)                        | `act::resolve_path` + delta repair in `apply_read_only`                                                                      |
| [x]    | Planner output logging                                   | `planner_logs/planner_iter_####_output.json` in `run_planner_execution_loop`                                                 |
| [x]    | Metrics snapshots                                        | `telemetry::record_snapshot` (capability + global + template)                                                                |

It gets stuck

archlinux in canon on  main                                                                                                                                                                                                 2026-03-30 12:50:20
❯ cargo run --bin canon-runtime-supervisor
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.49s
     Running `target/debug/canon-runtime-supervisor`
[llm-worker] relay server listening on 127.0.0.1:9101
[runtime][dedup_drop] kind=prompt_loaded id=b9d07a39-93b3-47a0-b9e6-2b43d72791ac — skipping consecutive duplicate
[policy][shared_constraint][input] route=plan rule=MissingTargetPlan target_missing=true validation_blocked=true compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=true planned_pending=0 invalid_plan_batches=0 planning_preconditions=1 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:missing_target_plan route=plan trigger=Some(EventId("654372e0-4ca5-47f7-8ee7-1ae2451ad8da")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=plan trigger=Some(EventId("654372e0-4ca5-47f7-8ee7-1ae2451ad8da")) last_control=None pending_succ=None
[tlog][pending_set] after_kind=loop_observed after_id=654372e0-4ca5-47f7-8ee7-1ae2451ad8da next_expected=Some("route_selected")
[tlog][pending_discharged] kind=route_selected id=75db79bc-6b5e-4d26-a53b-7ff1a5d36977 discharged_expected=route_selected after=loop_observed
[tlog][pending_set] after_kind=route_selected after_id=75db79bc-6b5e-4d26-a53b-7ff1a5d36977 next_expected=Some("planning_completed")
[policy][shared_constraint][input] route=act rule=PlannedToAct target_missing=true validation_blocked=true compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=true planned_pending=1 invalid_plan_batches=0 planning_preconditions=1 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:planned_to_act route=act trigger=Some(EventId("0b7052a8-071a-46f8-8884-c6713db79ef9")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=act trigger=Some(EventId("0b7052a8-071a-46f8-8884-c6713db79ef9")) last_control=None pending_succ=None
[tlog][pending_discharged] kind=planning_completed id=0b7052a8-071a-46f8-8884-c6713db79ef9 discharged_expected=planning_completed after=route_selected
[tlog][pending_set] after_kind=planning_completed after_id=0b7052a8-071a-46f8-8884-c6713db79ef9 next_expected=Some("route_selected")
[act_stage] exec_state target_root=/workspace/ai_sandbox/canon/test_projects/goalgen/event-sourcing-engine cwd=/workspace/ai_sandbox/canon/test_projects/goalgen real_cargo_toml=false
[tlog][pending_discharged] kind=route_selected id=a2e2cac8-7340-4547-96a1-11779cba632b discharged_expected=route_selected after=planning_completed
[tlog][pending_set] after_kind=route_selected after_id=a2e2cac8-7340-4547-96a1-11779cba632b next_expected=Some("loop_acted")
[runtime][dedup_drop] kind=runtime_state_updated id=e55a7426-89f2-4ffc-8dbb-e2cedbb3583d — skipping consecutive duplicate
[route_executor][det] rule=deterministic:bootstrap_refresh_observe route=observe trigger=Some(EventId("d500f69f-1107-440c-8fa2-da954455e1f7")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=observe trigger=Some(EventId("d500f69f-1107-440c-8fa2-da954455e1f7")) last_control=None pending_succ=None
[tlog][pending_discharged] kind=loop_acted id=d500f69f-1107-440c-8fa2-da954455e1f7 discharged_expected=loop_acted after=route_selected
[tlog][pending_set] after_kind=loop_acted after_id=d500f69f-1107-440c-8fa2-da954455e1f7 next_expected=Some("route_selected")
[tlog][pending_discharged] kind=route_selected id=767e5031-4b45-4f6a-8c2b-506cd2ebc11b discharged_expected=route_selected after=loop_acted
[tlog][pending_set] after_kind=route_selected after_id=767e5031-4b45-4f6a-8c2b-506cd2ebc11b next_expected=Some("loop_observed")
[event_repair_trigger] submitting workspace repair job to 127.0.0.1:9102
[event_repair_trigger] submit failed: Connection refused (os error 111)
[policy][shared_constraint][input] route=observe rule=BootstrapRefreshObserve target_missing=false validation_blocked=false compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=false planned_pending=0 invalid_plan_batches=0 planning_preconditions=0 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:bootstrap_refresh_observe route=observe trigger=Some(EventId("5159e837-b694-4a6c-a170-fea57f452b1e")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=observe trigger=Some(EventId("5159e837-b694-4a6c-a170-fea57f452b1e")) last_control=None pending_succ=None
[policy][shared_constraint][input] route=observe rule=BootstrapRefreshObserve target_missing=false validation_blocked=false compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=false planned_pending=0 invalid_plan_batches=0 planning_preconditions=0 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:bootstrap_refresh_observe route=observe trigger=Some(EventId("8b02b4a7-d6be-4979-9a8c-c715eb896f52")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=observe trigger=Some(EventId("8b02b4a7-d6be-4979-9a8c-c715eb896f52")) last_control=None pending_succ=None
[policy][shared_constraint][input] route=observe rule=BootstrapRefreshObserve target_missing=false validation_blocked=false compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=false planned_pending=0 invalid_plan_batches=0 planning_preconditions=0 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:bootstrap_refresh_observe route=observe trigger=Some(EventId("6246e66e-0242-4db5-b64b-6b53041a5b85")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=observe trigger=Some(EventId("6246e66e-0242-4db5-b64b-6b53041a5b85")) last_control=None pending_succ=None
[policy][shared_constraint][input] route=observe rule=BootstrapRefreshObserve target_missing=false validation_blocked=false compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=false planned_pending=0 invalid_plan_batches=0 planning_preconditions=0 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:bootstrap_refresh_observe route=observe trigger=Some(EventId("d43abc44-696f-44f0-9c85-e756196824ca")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=observe trigger=Some(EventId("d43abc44-696f-44f0-9c85-e756196824ca")) last_control=None pending_succ=None
[route_executor][det] rule=deterministic:router_llm_disabled_plan route=plan trigger=Some(EventId("e8b18c03-5775-4ae8-807e-fc1fefda3997")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=plan trigger=Some(EventId("e8b18c03-5775-4ae8-807e-fc1fefda3997")) last_control=None pending_succ=None
[tlog][pending_discharged] kind=loop_observed id=e8b18c03-5775-4ae8-807e-fc1fefda3997 discharged_expected=loop_observed after=route_selected
[tlog][pending_set] after_kind=loop_observed after_id=e8b18c03-5775-4ae8-807e-fc1fefda3997 next_expected=Some("route_selected")
[tlog][pending_discharged] kind=route_selected id=a340dd1c-c281-4be0-9f6c-2571af272c28 discharged_expected=route_selected after=loop_observed
[tlog][pending_set] after_kind=route_selected after_id=a340dd1c-c281-4be0-9f6c-2571af272c28 next_expected=Some("loop_observed")

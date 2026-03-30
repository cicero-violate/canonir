# INTENT
## Objective
Guarantee invariant-safe deterministic execution by enforcing one-to-one control event succession and preventing duplicate or invalid event emissions.
## Constraints
- no build break
- no test failure
## Targets
- event log writer (tlog / append / dedup)
- control event emitters (route executor / loop executor)
- invariant enforcement / validation layer
- event sequencing and scheduler
## Success Criteria
- zero invariant violations from duplicate or rejected events
- each control event produces exactly one valid successor
- event log appends succeed without rejection
- deterministic replay produces identical event sequences
- failing scenario reproduces before fix and passes after
[constraint][plan_stays_plan] route=plan deterministic_route=None failure_class_no_actionable=true recent_no_semantic_progress=false actionable_failure=false validation_blocked=false entrypoint_missing=false module_gaps_present=false real_path_exists=true real_cargo_project=true semantic_path_exists=true semantic_cargo_project=true
[policy][shared_constraint][input] route=act rule=PlannedToAct target_missing=false validation_blocked=false compiler_repair_required=false failure_class=Some("no_actionable_failure") failure_scope=Some("none") no_progress=false actionable_failure=false planned_pending=3 invalid_plan_batches=0 planning_preconditions=0 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:planned_to_act route=act trigger=Some(EventId("99a63dce-cf1a-426b-9fc2-55f4e094f7fc")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=act trigger=Some(EventId("99a63dce-cf1a-426b-9fc2-55f4e094f7fc")) last_control=None pending_succ=None
[tlog][pending_discharged] kind=planning_completed id=99a63dce-cf1a-426b-9fc2-55f4e094f7fc discharged_expected=planning_completed after=route_selected
[tlog][pending_set] after_kind=planning_completed after_id=99a63dce-cf1a-426b-9fc2-55f4e094f7fc next_expected=Some("route_selected")
[tlog][pending_discharged] kind=route_selected id=3b596c4d-5f28-43e3-8a44-43b0e50791eb discharged_expected=route_selected after=planning_completed
[tlog][pending_set] after_kind=route_selected after_id=3b596c4d-5f28-43e3-8a44-43b0e50791eb next_expected=Some("loop_acted")
[route_executor][det] rule=deterministic:no_actionable_failure_observe route=observe trigger=Some(EventId("2671893f-858c-4735-a1e7-8d6de84a7ae3")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=observe trigger=Some(EventId("2671893f-858c-4735-a1e7-8d6de84a7ae3")) last_control=None pending_succ=None
[tlog][pending_discharged] kind=loop_acted id=2671893f-858c-4735-a1e7-8d6de84a7ae3 discharged_expected=loop_acted after=route_selected
[tlog][pending_set] after_kind=loop_acted after_id=2671893f-858c-4735-a1e7-8d6de84a7ae3 next_expected=Some("route_selected")
[tlog][pending_discharged] kind=route_selected id=d4965737-73f6-4a42-81ad-86bc2f86430a discharged_expected=route_selected after=loop_acted
[tlog][pending_set] after_kind=route_selected after_id=d4965737-73f6-4a42-81ad-86bc2f86430a next_expected=Some("loop_observed")
[event_repair_trigger] submitting workspace repair job to 127.0.0.1:9102
[event_repair_trigger] submit failed: Connection refused (os error 111)
[tlog][pending_discharged] kind=loop_observed id=54bdb832-3ec8-40ad-b1c3-e2595c87eaff discharged_expected=loop_observed after=route_selected
[tlog][pending_set] after_kind=loop_observed after_id=54bdb832-3ec8-40ad-b1c3-e2595c87eaff next_expected=Some("route_selected")
[event_repair_trigger] cooldown active; skipping submit

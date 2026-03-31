Fix this issue

Don't run canon-runtime-supervisor, the program is LIVE

archlinux in canon on  main                                                                                                                                                                                                 2026-03-30 22:24:13
### Expanded Pending Tasks (Act-stage invariant + noop_spam)

- [x] Diagnose illegal PlanningCompleted → Act transition  ✓ done
  1. Run: rg -n "planned_to_act" canon-utils
  2. Open canon-utils/canon-route/src/executor.rs at matches
  3. Identify condition allowing route=act when planned_pending=0
  4. Verify absence of scheduler_len guard
  5. Record file:line of invalid Act emission

- [x] Block Act when scheduler is empty  ✓ done
  1. Open canon-utils/canon-route/src/executor.rs
  2. Locate all RouteKind::Act emission sites
  3. Add guard: if ctx.scheduler.is_empty() { return Observe }
  4. Add debug log: "[ROUTE] blocked Act (scheduler empty)"
  5. Add debug_assert!(ctx.scheduler.len() > 0)

- [x] Prevent PlanningCompleted without executable work  ✓ done
  1. Run: rg -n "PlanningCompleted" canon-utils
  2. Open canon-utils/canon-route/src/policy.rs
  3. Replace ctx.planned_pending with ctx.scheduler.len()
  4. If scheduler empty → return Observe
  5. Add log: "[PLAN] blocked PlanningCompleted scheduler_len=0"

- [x] Guard LoopActed emission in act.rs  ✓ done
  1. Open canon-utils/canon-loop/src/stage/act.rs around line 1311
  2. Locate emit_acted and all LoopActed emissions
  3. Require tool_result_id.is_some()
  4. If missing → return Observe
  5. Add log: "[ACT] blocked LoopActed (no tool_result)"

- [x] Remove loop_acted from Observe/noop paths  ✓ done
  1. Run: rg -n "loop_acted" canon-utils
  2. Identify bootstrap_refresh_observe paths
  3. Replace LoopActed with loop_observed or no-op
  4. Ensure Observe never emits LoopActed
  5. Add log: "[EXECUTOR] suppressed loop_acted (observe path)"

- [ ] Validate via logs
  1. Run: cargo run --bin canon-runtime-supervisor
  2. rg -n "NOOP_SPAM_TRACE" canon/state/log.txt → expect 0
  3. rg -n "LoopActed" canon/state/log.txt → all must have tool_result_id
  4. Ensure no panic "LoopActed emitted without tool_result_id"
  5. Verify full Observe→Plan→Act→ToolCall→ToolResult→Verify cycle
❯ cargo run --bin canon-runtime-supervisor
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
     Running `target/debug/canon-runtime-supervisor`
[llm-worker] relay server listening on 127.0.0.1:9101
[runtime][dedup_drop] kind=prompt_loaded id=c057f6b1-99fa-4529-b3c6-4c6aa1fe8361 — skipping consecutive duplicate
[policy][shared_constraint][input] route=plan rule=MissingTargetPlan target_missing=true validation_blocked=true compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=true planned_pending=0 invalid_plan_batches=0 planning_preconditions=1 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:missing_target_plan route=plan trigger=Some(EventId("4d6f7e29-f87e-4e1f-89d4-19bee52437a9")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=plan trigger=Some(EventId("4d6f7e29-f87e-4e1f-89d4-19bee52437a9")) last_control=None pending_succ=None
[tlog][pending_set] after_kind=loop_observed after_id=4d6f7e29-f87e-4e1f-89d4-19bee52437a9 next_expected=Some("route_selected")
[tlog][pending_discharged] kind=route_selected id=0582ba3e-0cdf-4de5-bfc9-262872d37502 discharged_expected=route_selected after=loop_observed
[tlog][pending_set] after_kind=route_selected after_id=0582ba3e-0cdf-4de5-bfc9-262872d37502 next_expected=Some("planning_completed")
[route_executor][det] rule=deterministic:planned_to_act route=act trigger=Some(EventId("fe145838-8e0f-4741-b4c5-f82709e496da")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=act trigger=Some(EventId("fe145838-8e0f-4741-b4c5-f82709e496da")) last_control=None pending_succ=None
[NOOP_SPAM_TRACE] event_id=fe145838-8e0f-4741-b4c5-f82709e496da kind=planning_completed reasons=["route_executor:route_policy_planned_to_act"]
[tlog][pending_discharged] kind=planning_completed id=fe145838-8e0f-4741-b4c5-f82709e496da discharged_expected=planning_completed after=route_selected
[tlog][pending_set] after_kind=planning_completed after_id=fe145838-8e0f-4741-b4c5-f82709e496da next_expected=Some("route_selected")
[act_stage] exec_state target_root=/workspace/ai_sandbox/canon/test_projects/goalgen/actor-system-sim cwd=/workspace/ai_sandbox/canon/test_projects/goalgen real_cargo_toml=false
[tlog][pending_discharged] kind=route_selected id=ecf24084-9bbc-4d58-8457-9d16788df102 discharged_expected=route_selected after=planning_completed
[tlog][pending_set] after_kind=route_selected after_id=ecf24084-9bbc-4d58-8457-9d16788df102 next_expected=Some("loop_acted")
[runtime][dedup_drop] kind=runtime_state_updated id=cb23cf94-c419-4456-a3e0-5dc4143b2613 — skipping consecutive duplicate
[route_executor][det] rule=deterministic:bootstrap_refresh_observe route=observe trigger=Some(EventId("e934df34-a032-4701-83ae-227403507684")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=observe trigger=Some(EventId("e934df34-a032-4701-83ae-227403507684")) last_control=None pending_succ=None
[NOOP_SPAM_TRACE] event_id=e934df34-a032-4701-83ae-227403507684 kind=loop_acted reasons=["route_executor:route_executor_bootstrap_refresh_observe"]
[tlog][pending_discharged] kind=loop_acted id=e934df34-a032-4701-83ae-227403507684 discharged_expected=loop_acted after=route_selected
[tlog][pending_set] after_kind=loop_acted after_id=e934df34-a032-4701-83ae-227403507684 next_expected=Some("route_selected")
[tlog][pending_discharged] kind=route_selected id=e8904e8a-743f-47f8-9c74-7c842e2f7e33 discharged_expected=route_selected after=loop_acted
[tlog][pending_set] after_kind=route_selected after_id=e8904e8a-743f-47f8-9c74-7c842e2f7e33 next_expected=Some("loop_observed")
[policy][shared_constraint][input] route=observe rule=BootstrapRefreshObserve target_missing=false validation_blocked=false compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=false planned_pending=0 invalid_plan_batches=0 planning_preconditions=0 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:bootstrap_refresh_observe route=observe trigger=Some(EventId("c7f5e596-d634-4b20-9740-80c989b1a6e4")) last_control=None pending_succ=None
[policy][shared_constraint][input] route=observe rule=BootstrapRefreshObserve target_missing=false validation_blocked=false compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=false planned_pending=0 invalid_plan_batches=0 planning_preconditions=0 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:bootstrap_refresh_observe route=observe trigger=Some(EventId("7e6a0448-1660-48a5-9f17-648ab1b340a0")) last_control=None pending_succ=None
[policy][shared_constraint][input] route=observe rule=BootstrapRefreshObserve target_missing=false validation_blocked=false compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=false planned_pending=0 invalid_plan_batches=0 planning_preconditions=0 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:bootstrap_refresh_observe route=observe trigger=Some(EventId("4a95ac7e-05c0-4ed2-84a7-dab3028efa82")) last_control=None pending_succ=None
[policy][shared_constraint][input] route=observe rule=BootstrapRefreshObserve target_missing=false validation_blocked=false compiler_repair_required=false failure_class=None failure_scope=None no_progress=false actionable_failure=false planned_pending=0 invalid_plan_batches=0 planning_preconditions=0 compiler_hints=0 module_gaps=0
[route_executor][det] rule=deterministic:bootstrap_refresh_observe route=observe trigger=Some(EventId("23b384e4-eac5-40b9-955d-3c69fe8cdb7a")) last_control=None pending_succ=None
[route_executor][det] rule=deterministic:router_llm_disabled_plan route=plan trigger=Some(EventId("b7517ea9-8a02-4943-9ba0-20a2dcf63ff6")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=plan trigger=Some(EventId("b7517ea9-8a02-4943-9ba0-20a2dcf63ff6")) last_control=None pending_succ=None
[tlog][pending_discharged] kind=loop_observed id=b7517ea9-8a02-4943-9ba0-20a2dcf63ff6 discharged_expected=loop_observed after=route_selected
[tlog][pending_set] after_kind=loop_observed after_id=b7517ea9-8a02-4943-9ba0-20a2dcf63ff6 next_expected=Some("route_selected")
[tlog][pending_discharged] kind=route_selected id=5ee753e3-4b6d-46ba-a760-a1d5b96f44c8 discharged_expected=route_selected after=loop_observed
[tlog][pending_set] after_kind=route_selected after_id=5ee753e3-4b6d-46ba-a760-a1d5b96f44c8 next_expected=Some("planning_completed")
[constraint][plan_stays_plan] route=plan deterministic_route=None failure_class_no_actionable=true recent_no_semantic_progress=false actionable_failure=false validation_blocked=false entrypoint_missing=false module_gaps_present=false real_path_exists=true real_cargo_project=true semantic_path_exists=true semantic_cargo_project=true
[tlog][dedup_drop] kind=debug id=25c954b1-f131-44f1-8fac-8e333082962b actor=loop_stage_dispatch content_hash_collision (non-control)
[route_executor][det] rule=deterministic:planned_to_act route=act trigger=Some(EventId("0299f36a-0cae-4f8f-8e9a-8116818fbc04")) last_control=None pending_succ=None
[route_executor][emit] route_selected lane=act trigger=Some(EventId("0299f36a-0cae-4f8f-8e9a-8116818fbc04")) last_control=None pending_succ=None
[NOOP_SPAM_TRACE] event_id=0299f36a-0cae-4f8f-8e9a-8116818fbc04 kind=planning_completed reasons=["route_executor:route_policy_planned_to_act"]
[tlog][pending_discharged] kind=planning_completed id=0299f36a-0cae-4f8f-8e9a-8116818fbc04 discharged_expected=planning_completed after=route_selected
[tlog][pending_set] after_kind=planning_completed after_id=0299f36a-0cae-4f8f-8e9a-8116818fbc04 next_expected=Some("route_selected")

thread 'main' (3673173) panicked at canon-utils/canon-loop/src/stage/act.rs:1311:5:
[ACT][INVARIANT] LoopActed emitted without tool_result_id (non-actionable)
stack backtrace:
   0:     0x59ff58a5079a - std[51bd67ac9af55117]::backtrace_rs::backtrace::libunwind::trace
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/../../backtrace/src/backtrace/libunwind.rs:117:9
   1:     0x59ff58a5079a - std[51bd67ac9af55117]::backtrace_rs::backtrace::trace_unsynchronized::<std[51bd67ac9af55117]::sys::backtrace::_print_fmt::{closure#1}>
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/../../backtrace/src/backtrace/mod.rs:66:14
   2:     0x59ff58a5079a - std[51bd67ac9af55117]::sys::backtrace::_print_fmt
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/sys/backtrace.rs:74:9
   3:     0x59ff58a5079a - <<std[51bd67ac9af55117]::sys::backtrace::BacktraceLock>::print::DisplayBacktrace as core[28bfa958bd7f7b14]::fmt::Display>::fmt
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/sys/backtrace.rs:44:26
   4:     0x59ff58a6999a - <core[28bfa958bd7f7b14]::fmt::rt::Argument>::fmt
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/core/src/fmt/rt.rs:152:76
   5:     0x59ff58a6999a - core[28bfa958bd7f7b14]::fmt::write
   6:     0x59ff58a56462 - std[51bd67ac9af55117]::io::default_write_fmt::<std[51bd67ac9af55117]::sys::stdio::unix::Stderr>
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/io/mod.rs:639:11
   7:     0x59ff58a56462 - <std[51bd67ac9af55117]::sys::stdio::unix::Stderr as std[51bd67ac9af55117]::io::Write>::write_fmt
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/io/mod.rs:1994:13
   8:     0x59ff58a2b12f - <std[51bd67ac9af55117]::sys::backtrace::BacktraceLock>::print
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/sys/backtrace.rs:47:9
   9:     0x59ff58a2b12f - std[51bd67ac9af55117]::panicking::default_hook::{closure#0}
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panicking.rs:292:27
  10:     0x59ff58a47351 - std[51bd67ac9af55117]::panicking::default_hook
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panicking.rs:319:9
  11:     0x59ff58a475cb - std[51bd67ac9af55117]::panicking::panic_with_hook
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panicking.rs:825:13
  12:     0x59ff58a2b21a - std[51bd67ac9af55117]::panicking::panic_handler::{closure#0}
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panicking.rs:691:13
  13:     0x59ff58a1fa19 - std[51bd67ac9af55117]::sys::backtrace::__rust_end_short_backtrace::<std[51bd67ac9af55117]::panicking::panic_handler::{closure#0}, !>
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/sys/backtrace.rs:182:18
  14:     0x59ff58a2c27d - __rustc[7555343a6f564530]::rust_begin_unwind
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panicking.rs:689:5
  15:     0x59ff58a6a2fc - core[28bfa958bd7f7b14]::panicking::panic_fmt
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/core/src/panicking.rs:80:14
  16:     0x59ff57afb4dc - canon_loop[a9bbdd6178bff518]::stage::act::emit_acted
                               at /workspace/ai_sandbox/canon/canon-utils/canon-loop/src/stage/act.rs:1311:5
  17:     0x59ff57afe832 - canon_loop[a9bbdd6178bff518]::stage::act::dispatch_plan
                               at /workspace/ai_sandbox/canon/canon-utils/canon-loop/src/stage/act.rs:1218:25
  18:     0x59ff57b1c8b3 - canon_loop[a9bbdd6178bff518]::stage::act::execute_dispatch
                               at /workspace/ai_sandbox/canon/canon-utils/canon-loop/src/stage/act.rs:91:5
  19:     0x59ff57aa8045 - <canon_loop[a9bbdd6178bff518]::stage::LoopStageEvent>::execute
                               at /workspace/ai_sandbox/canon/canon-utils/canon-loop/src/stage/mod.rs:29:47
  20:     0x59ff57a5b423 - <canon_loop[a9bbdd6178bff518]::executor::LoopStageExecutor>::execute_stage_event
                               at /workspace/ai_sandbox/canon/canon-utils/canon-loop/src/executor.rs:713:25
  21:     0x59ff57a64a50 - <canon_loop[a9bbdd6178bff518]::executor::LoopStageExecutor as canon_event[62a791fe865d7644]::events::EventConsumer>::on_event
                               at /workspace/ai_sandbox/canon/canon-utils/canon-loop/src/executor.rs:855:14
  22:     0x59ff5789599c - <canon_runtime[1479fd936e83bdeb]::bus::EventBus>::dispatch
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/bus.rs:196:38
  23:     0x59ff57860ee7 - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::handle_runtime_event_located_with_parents
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:445:39
  24:     0x59ff5785deae - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::drain_emitted_events
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:534:18
  25:     0x59ff57863099 - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::handle_runtime_event_located_with_parents
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:517:14
  26:     0x59ff5785deae - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::drain_emitted_events
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:534:18
  27:     0x59ff57863099 - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::handle_runtime_event_located_with_parents
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:517:14
  28:     0x59ff5785deae - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::drain_emitted_events
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:534:18
  29:     0x59ff57863099 - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::handle_runtime_event_located_with_parents
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:517:14
  30:     0x59ff5785deae - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::drain_emitted_events
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:534:18
  31:     0x59ff57863099 - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::handle_runtime_event_located_with_parents
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:517:14
  32:     0x59ff5785deae - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::drain_emitted_events
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:534:18
  33:     0x59ff57863099 - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::handle_runtime_event_located_with_parents
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:517:14
  34:     0x59ff5785deae - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::drain_emitted_events
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:534:18
  35:     0x59ff57863099 - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::handle_runtime_event_located_with_parents
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:517:14
  36:     0x59ff5785deae - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::drain_emitted_events
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:534:18
  37:     0x59ff57863099 - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::handle_runtime_event_located_with_parents
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:517:14
  38:     0x59ff5785deae - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::drain_emitted_events
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:534:18
  39:     0x59ff5785df2a - <canon_runtime[1479fd936e83bdeb]::EventRuntime>::flush_emitted_events
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/lib.rs:317:14
  40:     0x59ff5779efe0 - canon_runtime[e0b7aef76a9794ce]::main
                               at /workspace/ai_sandbox/canon/canon-utils/canon-runtime/src/bin/event_runtime.rs:613:17
  41:     0x59ff577b270b - <fn() -> core[28bfa958bd7f7b14]::result::Result<(), anyhow[2e19385b00c4b233]::Error> as core[28bfa958bd7f7b14]::ops::function::FnOnce<()>>::call_once
                               at /home/cicero-arch-omen/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/core/src/ops/function.rs:250:5
  42:     0x59ff577b2d5d - std[51bd67ac9af55117]::sys::backtrace::__rust_begin_short_backtrace::<fn() -> core[28bfa958bd7f7b14]::result::Result<(), anyhow[2e19385b00c4b233]::Error>, core[28bfa958bd7f7b14]::result::Result<(), anyhow[2e19385b00c4b233]::Error>>
                               at /home/cicero-arch-omen/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/std/src/sys/backtrace.rs:166:18
  43:     0x59ff57789251 - std[51bd67ac9af55117]::rt::lang_start::<core[28bfa958bd7f7b14]::result::Result<(), anyhow[2e19385b00c4b233]::Error>>::{closure#0}
                               at /home/cicero-arch-omen/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/std/src/rt.rs:206:18
  44:     0x59ff58a45b94 - <&dyn core[28bfa958bd7f7b14]::ops::function::Fn<(), Output = i32> + core[28bfa958bd7f7b14]::panic::unwind_safe::RefUnwindSafe + core[28bfa958bd7f7b14]::marker::Sync as core[28bfa958bd7f7b14]::ops::function::FnOnce<()>>::call_once
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/core/src/ops/function.rs:287:21
  45:     0x59ff58a45b94 - std[51bd67ac9af55117]::panicking::catch_unwind::do_call::<&dyn core[28bfa958bd7f7b14]::ops::function::Fn<(), Output = i32> + core[28bfa958bd7f7b14]::panic::unwind_safe::RefUnwindSafe + core[28bfa958bd7f7b14]::marker::Sync, i32>
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panicking.rs:581:40
  46:     0x59ff58a45b94 - std[51bd67ac9af55117]::panicking::catch_unwind::<i32, &dyn core[28bfa958bd7f7b14]::ops::function::Fn<(), Output = i32> + core[28bfa958bd7f7b14]::panic::unwind_safe::RefUnwindSafe + core[28bfa958bd7f7b14]::marker::Sync>
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panicking.rs:544:19
  47:     0x59ff58a45b94 - std[51bd67ac9af55117]::panic::catch_unwind::<&dyn core[28bfa958bd7f7b14]::ops::function::Fn<(), Output = i32> + core[28bfa958bd7f7b14]::panic::unwind_safe::RefUnwindSafe + core[28bfa958bd7f7b14]::marker::Sync, i32>
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panic.rs:359:14
  48:     0x59ff58a45b94 - std[51bd67ac9af55117]::rt::lang_start_internal::{closure#0}
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/rt.rs:175:24
  49:     0x59ff58a45b94 - std[51bd67ac9af55117]::panicking::catch_unwind::do_call::<std[51bd67ac9af55117]::rt::lang_start_internal::{closure#0}, isize>
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panicking.rs:581:40
  50:     0x59ff58a45b94 - std[51bd67ac9af55117]::panicking::catch_unwind::<isize, std[51bd67ac9af55117]::rt::lang_start_internal::{closure#0}>
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panicking.rs:544:19
  51:     0x59ff58a45b94 - std[51bd67ac9af55117]::panic::catch_unwind::<std[51bd67ac9af55117]::rt::lang_start_internal::{closure#0}, isize>
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/panic.rs:359:14
  52:     0x59ff58a45b94 - std[51bd67ac9af55117]::rt::lang_start_internal
                               at /rustc/2d76d9bc76f27b03b4899e72ce561c7ac2c5cf6b/library/std/src/rt.rs:171:5
  53:     0x59ff57789237 - std[51bd67ac9af55117]::rt::lang_start::<core[28bfa958bd7f7b14]::result::Result<(), anyhow[2e19385b00c4b233]::Error>>
                               at /home/cicero-arch-omen/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/std/src/rt.rs:205:5
  54:     0x59ff577a1b1e - main
  55:     0x7841161186c1 - <unknown>
  56:     0x7841161187f9 - __libc_start_main
  57:     0x59ff577753b5 - _start
  58:                0x0 - <unknown>

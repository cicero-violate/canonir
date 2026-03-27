use crate::stage::observe;
use crate::{
    context::LoopContext,
    result::LoopStageResult,
    scheduler::{infer_priority, ScheduledTask},
    stage::LoopStageEvent,
};
use canon_event::{AgentRegistered, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, GoalEdgeDefined, RuntimeEvent, Tick};
use canon_invariant::decision_trace_payload;
use canon_proc_macros::must_emit;
use std::path::PathBuf;
use std::time::Instant;

pub struct LoopStageExecutor {
    ctx: LoopContext,
}

impl LoopStageExecutor {
    pub fn new(workspace: PathBuf, tlog_path: PathBuf) -> Self {
        Self { ctx: LoopContext::new(workspace, tlog_path) }
    }

    pub fn with_agent_id(mut self, id: String) -> Self {
        self.ctx.agent_id = Some(id);
        self
    }

    fn record_control_state(&mut self, event: &RuntimeEvent, trigger_id: &EventId) {
        let next = match event {
            RuntimeEvent::RouteSelected(rs) => match rs.approved_route.as_str() {
                "observe" => Some("loop_observed"),
                "plan" => Some("planning_completed"),
                "act" => Some("loop_acted"),
                "verify" => Some("loop_verified"),
                "conclude" => Some("loop_rewarded"),
                _ => None,
            },
            RuntimeEvent::LoopObserved(_) => Some("route_selected"),
            RuntimeEvent::PlanningCompleted(_) => Some("route_selected"),
            RuntimeEvent::LoopActed(_) => Some("route_selected"),
            RuntimeEvent::LoopVerified(_) => Some("loop_rewarded"),
            RuntimeEvent::LoopRewarded(_) => Some("route_selected"),
            _ => None,
        };
        if let Some(expected) = next {
            self.ctx.last_control_event_id = Some(trigger_id.to_string());
            self.ctx.last_control_kind = Some(canon_event::event_kind_str(event).to_string());
            self.ctx.pending_required_successor = Some(expected.to_string());
        }
    }

    fn is_successful_bootstrap_action(a: &canon_event::LoopActed) -> bool {
        if a.action_kind != "run_command" || !a.success {
            return false;
        }
        let text = if !a.stdout.is_empty() { &a.stdout } else { &a.stderr };
        text.contains("Creating binary (application) package")
            || text.contains("Creating library package")
            || text.contains("Creating binary (application) `")
            || text.contains("Creating library `")
    }
}

impl EventConsumer for LoopStageExecutor {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn consumer_name(&self) -> &'static str {
        "loop_stage_executor"
    }

    fn is_synchronous(&self) -> bool {
        true
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.ctx.emitter = Some(emitter);
    }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        if let RuntimeEvent::Debug(debug) = event {
            if debug.kind == "recovery_event" {
                let expected = debug
                    .payload
                    .get("context")
                    .and_then(|v| v.get("expected_successor"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                if let Some(emitter) = self.ctx.emitter.as_ref() {
                    emitter.emit_child(
                        RuntimeEvent::Debug(canon_event::DebugEvent {
                            source: "loop_stage_executor".to_string(),
                            kind: "recovery_received".to_string(),
                            payload: decision_trace_payload(
                                "recovery event received by loop stage executor",
                                serde_json::json!({
                                    "expected_successor": expected,
                                    "trigger_kind": canon_event::event_kind_str(event),
                                }),
                            ),
                        }),
                        vec![trigger_id.clone()],
                        file!(),
                        line!(),
                    );
                }

                if expected == "loop_rewarded" {
                    if self.ctx.pending_required_successor.as_deref() != Some("loop_rewarded") {
                        if let Some(emitter) = self.ctx.emitter.as_ref() {
                            emitter.emit_child(
                                RuntimeEvent::Debug(canon_event::DebugEvent {
                                    source: "loop_stage_executor".to_string(),
                                    kind: "reward_recovery_skipped_already_satisfied".to_string(),
                                    payload: decision_trace_payload(
                                        "reward recovery skipped because loop_rewarded is no longer the pending successor",
                                        serde_json::json!({
                                            "pending_required_successor": self.ctx.pending_required_successor,
                                            "last_control_kind": self.ctx.last_control_kind,
                                            "last_control_event_id": self.ctx.last_control_event_id,
                                        }),
                                    ),
                                }),
                                vec![trigger_id.clone()],
                                file!(),
                                line!(),
                            );
                        }
                        return EventOutcome::NoOp("reward_recovery_already_satisfied");
                    }
                    let Some(last_verified) = self.ctx.last_verified.clone() else {
                        if let Some(emitter) = self.ctx.emitter.as_ref() {
                            emitter.emit_child(
                                RuntimeEvent::Debug(canon_event::DebugEvent {
                                    source: "loop_stage_executor".to_string(),
                                    kind: "reward_recovery_missing_context".to_string(),
                                    payload: decision_trace_payload(
                                        "reward recovery requested but no last verified event is available",
                                        serde_json::json!({
                                            "trigger_kind": canon_event::event_kind_str(event),
                                        }),
                                    ),
                                }),
                                vec![trigger_id.clone()],
                                file!(),
                                line!(),
                            );
                            emitter.emit_child(
                                RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(
                                    "reward_stall",
                                    "loop_stage_executor",
                                    "reward recovery requested without last_verified context".to_string(),
                                    "warning",
                                    serde_json::json!({ "recoverable": true }),
                                    None,
                                )),
                                vec![trigger_id.clone()],
                                file!(),
                                line!(),
                            );
                        }
                        return EventOutcome::NoOp("reward_recovery_missing_context");
                    };
                    match crate::stage::reward::execute(last_verified, &mut self.ctx) {
                        Ok(LoopStageResult::Emit(e)) => {
                            if let Some(emitter) = self.ctx.emitter.as_ref() {
                                emitter.emit_with_parents(e, vec![trigger_id.clone()], file!(), line!());
                            }
                        }
                        Ok(LoopStageResult::EmitMany(evs)) => {
                            if let Some(emitter) = self.ctx.emitter.as_ref() {
                                for event in evs {
                                    emitter.emit_with_parents(event, vec![trigger_id.clone()], file!(), line!());
                                }
                            }
                        }
                        Ok(LoopStageResult::Deferred) | Ok(LoopStageResult::Noop) => {
                            if let Some(emitter) = self.ctx.emitter.as_ref() {
                                emitter.emit_child(
                                    RuntimeEvent::Debug(canon_event::DebugEvent {
                                        source: "loop_stage_executor".to_string(),
                                        kind: "reward_recovery_noop".to_string(),
                                        payload: decision_trace_payload(
                                            "reward recovery produced no events",
                                            serde_json::json!({}),
                                        ),
                                    }),
                                    vec![trigger_id.clone()],
                                    file!(),
                                    line!(),
                                );
                            }
                        }
                        Err(err) => {
                            if let Some(emitter) = self.ctx.emitter.as_ref() {
                                emitter.emit_child(
                                    RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(
                                        "reward_recovery_execution",
                                        "loop_stage_executor",
                                        err.to_string(),
                                        "error",
                                        serde_json::json!({ "event": canon_event::event_kind_str(event) }),
                                        None,
                                    )),
                                    vec![trigger_id.clone()],
                                    file!(),
                                    line!(),
                                );
                            }
                        }
                    }
                    return EventOutcome::NoOp("reward_recovery_executed");
                }
            }
        }
        // State accumulation (mutations that do not emit).
        let mut trigger_observe = false;
        let mut force_observe_recovery = false;
        let mut force_reward_recovery = false;
        match event {
            RuntimeEvent::Debug(debug) if debug.kind == "recovery_event" => {
                match debug
                    .payload
                    .get("context")
                    .and_then(|v| v.get("expected_successor"))
                    .and_then(|v| v.as_str())
                {
                    Some("loop_observed") => force_observe_recovery = true,
                    Some("loop_rewarded") => force_reward_recovery = true,
                    _ => {}
                }
            }
            RuntimeEvent::RouteSelected(rs) => {
                self.ctx.last_route_rationale = Some(rs.rationale.clone());
                self.ctx.last_route_confidence = rs.confidence.map(|c| c as f64);
                if !rs.rationale.is_empty() {
                    self.ctx.last_route_rationale_non_empty = Some(rs.rationale.clone());
                    self.ctx.last_route_confidence_non_empty = rs.confidence.map(|c| c as f64);
                }
            }
            RuntimeEvent::Tick(Tick { tick, .. }) => {
                // Tick is kept for cursor-save bookkeeping but no longer drives observation
                // or timeout checks — the system is purely event-driven.
                self.ctx.current_tick = *tick;
            }
            RuntimeEvent::AgentRegistered(AgentRegistered { payload }) => {
                if let Some(id) = payload.get("agent_id").and_then(|v| v.as_str()) {
                    let cap = payload.get("capacity").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                    self.ctx.scheduler.agent_capacity.insert(id.to_string(), cap);
                }
            }
            RuntimeEvent::LoopObserved(o) => {
                self.ctx.last_observed = Some(o.clone());
                self.ctx.last_observed_tick = Some(o.tick);
                self.ctx.errors_before = o.error_count;
            }
            RuntimeEvent::LoopActed(a) => {
                self.ctx.last_acted = Some(a.clone());
                self.ctx.last_action_kind = a.action_kind.clone();
                self.ctx.last_action_success = a.success;
                self.ctx.batch_acted.push(a.clone());
                self.ctx.last_planned_observed_tick = None;
                // Clear so the planner re-plans after each action cycle.
                // batch_tool_results (accumulated since last plan) carry the new context.
                // Without this, the hash guard blocks re-planning with the same base observation.
                self.ctx.last_handled_observed_hash = None;
                self.ctx.last_emitted_plan_hash = None;
                self.ctx.last_delta_hash = None;
                self.ctx.consecutive_invalid_plan_batches = 0;
                self.ctx.last_invalid_plan_reason = None;
                self.ctx.last_invalid_plan_planned_count = None;
                if let Some(action_id) = a.action_id.clone() {
                    if let Some(paths) = self.ctx.write_paths_by_action.remove(&action_id) {
                        for p in paths {
                            self.ctx.file_write_tracker.release(&p);
                        }
                    }
                    // Mark workspace dirty for mutating actions.
                    const READ_ONLY_ACTIONS: &[&str] = &["list_dir", "read_file", "search_files", "done"];
                    if !READ_ONLY_ACTIONS.contains(&a.action_kind.as_str()) {
                        self.ctx.dirty_tracker.mark_dirty("orchestrator", Some(&action_id));
                    }
                    // Release dependency waits — by UUID action_id first, then by
                    // action_kind name so LLM-generated depends_on strings like
                    // ["read_file", "apply_patch"] resolve correctly.
                    for task in self.ctx.dep_tracker.complete(&action_id) {
                        self.ctx.scheduler.push(task);
                    }
                }
                // Also signal by action_kind (LLM uses human-readable names in depends_on).
                for task in self.ctx.dep_tracker.complete(&a.action_kind) {
                    self.ctx.scheduler.push(task);
                }
                if Self::is_successful_bootstrap_action(a) {
                    self.ctx.scheduler.clear();
                    self.ctx.dep_tracker.clear();
                    self.ctx.pending_act = None;
                    self.ctx.active_batch_llm_request_id = None;
                    self.ctx.act_batch_tracker.clear();
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit_child(
                            RuntimeEvent::Debug(canon_event::DebugEvent {
                                source: "loop_stage_executor".to_string(),
                                kind: "bootstrap_refresh_required".to_string(),
                                payload: decision_trace_payload(
                                    "successful bootstrap invalidated queued plan work; forcing a fresh observe is required",
                                    serde_json::json!({
                                        "action_kind": a.action_kind,
                                        "success": a.success,
                                        "target_workspace": self.ctx
                                            .goal_text
                                            .as_deref()
                                            .and_then(|t| canon_goal::parse_agent_goal_markdown(t).target_path)
                                            .map(|p| p.display().to_string()),
                                    }),
                                ),
                            }),
                            vec![trigger_id.clone()],
                            file!(),
                            line!(),
                        );
                    }
                }
            }
            RuntimeEvent::LoopVerified(v) => {
                self.ctx.last_verified = Some(v.clone());
                self.ctx.last_verify_trace_id = v.trace_id.clone();
                self.ctx.last_verify_execution_id = v.execution_id.clone();
                if v.passed {
                    self.ctx.error_count = 0;
                    self.ctx.warning_count = 0;
                    self.ctx.dirty_tracker.mark_verified("orchestrator");
                }
                // Allow re-planning after a verify/reward cycle: the reward stage may
                // route back to "plan" even if the base observation hash hasn't changed.
                self.ctx.last_handled_observed_hash = None;
                self.ctx.last_planned_observed_tick = None;
                self.ctx.last_emitted_plan_hash = None;
                self.ctx.last_delta_hash = None;
            }
            RuntimeEvent::SubTaskResult(r) => {
                self.ctx.context_merger.absorb(r, &r.agent_id);
            }
            RuntimeEvent::LoopPlanned(p) => {
                let priority = infer_priority(p, self.ctx.goodness, self.ctx.delta_g);
                let task = ScheduledTask { priority, enqueued_at: Instant::now(), seq: 0, agent_id: None, plan: p.clone() };
                if !p.depends_on.is_empty() {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        if let Some(child) = p.action_id.as_ref() {
                            for dep in &p.depends_on {
                                emitter.emit_child(RuntimeEvent::GoalEdgeDefined(GoalEdgeDefined { from_node_id: dep.clone(), to_node_id: child.clone(), created: true }), vec![trigger_id.clone()], file!(), line!());
                            }
                        }
                    }
                    self.ctx.dep_tracker.add(task);
                } else {
                    self.ctx.scheduler.push(task);
                }
            }
            RuntimeEvent::LoopRewarded(r) => {
                if r.halt {
                    self.ctx.halted = true;
                    let reason = if self.ctx.last_action_kind == "conclude" {
                        "explicit conclude route".to_string()
                    } else {
                        format!(
                            "loop_rewarded requested halt; reward={} stagnant_ticks={} errors_before={} errors_after={}",
                            r.reward, r.stagnant_ticks, r.errors_before, r.errors_after
                        )
                    };
                    self.ctx.last_halt_reason = Some(reason.clone());
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit_child(
                            RuntimeEvent::Debug(canon_event::DebugEvent {
                                source: "loop_stage_executor".to_string(),
                                kind: "loop_halt_reason".to_string(),
                                payload: decision_trace_payload(
                                    "loop entered halted state",
                                    serde_json::json!({
                                        "reason": reason,
                                        "tick": r.tick,
                                        "reward": r.reward,
                                        "stagnant_ticks": r.stagnant_ticks,
                                        "errors_before": r.errors_before,
                                        "errors_after": r.errors_after,
                                    }),
                                ),
                            }),
                            vec![trigger_id.clone()],
                            file!(),
                            line!(),
                        );
                    }
                }
            }
            RuntimeEvent::PlanningCompleted(pc) => {
                if pc.status == "invalid_plan" {
                    // Invalid plan is recoverable: clear planner suppression cursors so the
                    // same observed state/tick can immediately re-enter planning.
                    self.ctx.last_planned_observed_tick = None;
                    self.ctx.last_handled_observed_hash = None;
                    self.ctx.last_emitted_plan_hash = None;
                    self.ctx.last_delta_hash = None;
                } else {
                    self.ctx.consecutive_invalid_plan_batches = 0;
                    self.ctx.last_invalid_plan_reason = None;
                    self.ctx.last_invalid_plan_planned_count = None;
                }
            }
            RuntimeEvent::GoodnessSnapshot(g) => {
                self.ctx.goodness = Some(g.g);
                self.ctx.delta_g = Some(g.delta_g);
            }
            RuntimeEvent::RuntimeStateUpdated(updated) => {
                if updated.payload.get("fatal_invariant").and_then(|v| v.as_bool()).unwrap_or(false) {
                    self.ctx.halted = true;
                    self.ctx.last_halt_reason = updated
                        .payload
                        .get("fatal_invariant_reason")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string());
                } else if updated.payload.get("runtime_mode").and_then(|v| v.as_str()) == Some("running") {
                    self.ctx.halted = false;
                    self.ctx.last_halt_reason = None;
                }
            }
            RuntimeEvent::PromptLoaded(prompt) => {
                let data = prompt.payload.get("data").unwrap_or(&prompt.payload);
                if let Some(content) = data.get("content").and_then(|c| c.as_str()) {
                    self.ctx.goal_text = Some(content.to_string());
                    self.ctx.last_prompted_goal = Some(content.to_string());
                    trigger_observe = true;
                }
            }
            RuntimeEvent::ErrorOccurred(err) => {
                if err.severity == "warning" {
                    self.ctx.warning_count = self.ctx.warning_count.saturating_add(1);
                } else {
                    self.ctx.error_count = self.ctx.error_count.saturating_add(1);
                }
                self.ctx.recent_compiler_errors.push(serde_json::json!({
                    "reason": "error_occurred",
                    "message": {
                        "level": err.severity,
                        "message": err.message,
                    }
                }));
                if self.ctx.recent_compiler_errors.len() > 16 {
                    let drop_n = self.ctx.recent_compiler_errors.len() - 16;
                    self.ctx.recent_compiler_errors.drain(0..drop_n);
                }
                if err.kind == "invalid_plan_batch" {
                    self.ctx.consecutive_invalid_plan_batches =
                        self.ctx.consecutive_invalid_plan_batches.saturating_add(1);
                    self.ctx.last_invalid_plan_reason = Some(err.message.clone());
                    self.ctx.last_invalid_plan_planned_count = err
                        .context
                        .get("planned_count")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize);
                }
                let fatal_invariant_diag = err.kind == "diagnostics_triggered"
                    && err
                        .context
                        .get("fatal_invariant")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                if err.kind != "invariant_violation"
                    && err.kind != "invalid_plan_batch"
                    && !fatal_invariant_diag
                {
                    trigger_observe = true;
                }
            }
            RuntimeEvent::ToolResult(r) if r.kind != "llm.plan" => {
                self.ctx.batch_tool_results.push(r.clone());
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            | RuntimeEvent::RequestDispatch(_)
            | RuntimeEvent::Analysis(_)
            | RuntimeEvent::NodeReady(_)
            | RuntimeEvent::NodeStarted(_)
            | RuntimeEvent::NodeCompleted(_)
            | RuntimeEvent::NodeFailed(_)
            | RuntimeEvent::CapabilityCompleted(_)
            | RuntimeEvent::CapabilityFailed(_)
            | RuntimeEvent::PolicyBaselineUpdated(_)
            | RuntimeEvent::GoalSelected(_)
            | RuntimeEvent::SystemConfigLoaded(_)
            | RuntimeEvent::ToolCall(_)
            | RuntimeEvent::ToolResult(_)
            | RuntimeEvent::ToolBatchSettled(_)
            | RuntimeEvent::GoalNodeCreated(_)
            | RuntimeEvent::GoalNodeRetracted(_)
            | RuntimeEvent::GoalNodeRewritten(_)
            | RuntimeEvent::GoalEdgeDefined(_)
            | RuntimeEvent::GoalGraphCheckpointed(_)
            | RuntimeEvent::CapabilityInvoked(_)
            | RuntimeEvent::CapabilityResolved(_)
            | RuntimeEvent::InvariantDiscovered(_) => {}
        }
        self.record_control_state(event, &trigger_id);

        // Emit LoopObserved when state changes (event-driven, not tick-driven).
        let suppress_observe_on_invariant =
            matches!(event, RuntimeEvent::ErrorOccurred(err) if err.kind == "invariant_violation")
                || matches!(event, RuntimeEvent::Code(code) if matches!(code.delta.event, canon_event::RustcEvent::InvariantViolation(_)));

        let observe_blocked_by_successor = self
            .ctx
            .pending_required_successor
            .as_deref()
            .is_some_and(|expected| expected != "loop_observed");

        if force_observe_recovery && !self.ctx.halted {
            match observe::execute_forced(&mut self.ctx) {
                Ok(LoopStageResult::Emit(e)) => {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit_with_parents(e, vec![trigger_id.clone()], file!(), line!());
                    }
                }
                Ok(LoopStageResult::EmitMany(evs)) => {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        for event in evs {
                            emitter.emit_with_parents(event, vec![trigger_id.clone()], file!(), line!());
                        }
                    }
                }
                Ok(LoopStageResult::Deferred) | Ok(LoopStageResult::Noop) => {}
                Err(err) => {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit_child(
                            RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(
                                "observe_recovery_execution",
                                "loop_stage_executor",
                                err.to_string(),
                                "error",
                                serde_json::json!({ "event": canon_event::event_kind_str(event) }),
                                None,
                            )),
                            vec![trigger_id.clone()],
                            file!(),
                            line!(),
                        );
                    }
                }
            }
        } else if force_reward_recovery && !self.ctx.halted {
            // Handled eagerly for recovery_event before generic processing.
        } else if trigger_observe && !self.ctx.halted && !suppress_observe_on_invariant {
            if observe_blocked_by_successor {
                if let Some(emitter) = self.ctx.emitter.as_ref() {
                    emitter.emit_child(
                        RuntimeEvent::Debug(canon_event::DebugEvent {
                            source: "loop_stage_executor".to_string(),
                            kind: "observe_suppressed_due_to_pending_successor".to_string(),
                            payload: decision_trace_payload(
                                "observe suppressed because another control successor is required",
                                serde_json::json!({
                                    "pending_required_successor": self.ctx.pending_required_successor,
                                    "last_control_kind": self.ctx.last_control_kind,
                                    "last_control_event_id": self.ctx.last_control_event_id,
                                    "trigger_kind": canon_event::event_kind_str(event),
                                }),
                            ),
                        }),
                        vec![trigger_id.clone()],
                        file!(),
                        line!(),
                    );
                }
            } else {
            match observe::execute(&mut self.ctx) {
                Ok(LoopStageResult::Emit(e)) => {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit_with_parents(e, vec![trigger_id.clone()], file!(), line!());
                    }
                }
                Ok(LoopStageResult::EmitMany(evs)) => {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        for event in evs {
                            emitter.emit_with_parents(event, vec![trigger_id.clone()], file!(), line!());
                        }
                    }
                }
                Ok(LoopStageResult::Deferred) => {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit_child(RuntimeEvent::Debug(canon_event::DebugEvent { source: "loop_stage_executor".to_string(), kind: "observe_deferred".to_string(), payload: decision_trace_payload("observe returned deferred", serde_json::json!({ "trigger_kind": canon_event::event_kind_str(event) })) }), vec![trigger_id.clone()], file!(), line!());
                    }
                }
                Ok(LoopStageResult::Noop) => {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit_child(RuntimeEvent::Debug(canon_event::DebugEvent { source: "loop_stage_executor".to_string(), kind: "observe_noop".to_string(), payload: decision_trace_payload("observe returned noop", serde_json::json!({ "trigger_kind": canon_event::event_kind_str(event) })) }), vec![trigger_id.clone()], file!(), line!());
                    }
                }
                Err(err) => {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit_child(RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(
                            "observe_stage_execution",
                            "loop_stage_executor",
                            err.to_string(),
                            "error",
                            serde_json::json!({ "event": format!("{:?}", event) }),
                            None,
                        )), vec![trigger_id.clone()], file!(), line!());
                    }
                }
            }
            }
        }

        if self.ctx.halted {
            if matches!(event, RuntimeEvent::RouteSelected(_)) {
                if let Some(emitter) = self.ctx.emitter.as_ref() {
                    emitter.emit_child(
                        RuntimeEvent::Debug(canon_event::DebugEvent {
                            source: "loop_stage_executor".to_string(),
                            kind: "loop_stage_blocked".to_string(),
                            payload: decision_trace_payload(
                                "loop stage execution blocked because context is halted",
                                serde_json::json!({
                                    "event_kind": canon_event::event_kind_str(event),
                                    "last_halt_reason": self.ctx.last_halt_reason,
                                }),
                            ),
                        }),
                        vec![trigger_id.clone()],
                        file!(),
                        line!(),
                    );
                    emitter.emit_child(
                        RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(
                            "loop_stage_halted",
                            "loop_stage_executor",
                            "route_selected received while loop context is halted".to_string(),
                            "warning",
                            serde_json::json!({
                                "event_kind": canon_event::event_kind_str(event),
                                "last_halt_reason": self.ctx.last_halt_reason,
                                "recoverable": true,
                            }),
                            None,
                        )),
                        vec![trigger_id.clone()],
                        file!(),
                        line!(),
                    );
                }
            }
            return EventOutcome::NoOp("loop_stage_halted");
        }

        let Ok(stage) = LoopStageEvent::try_from(event.clone()) else {
            return EventOutcome::NoOp("loop_stage_not_stage_event");
        };
        let res = stage.execute(&mut self.ctx, trigger_id.clone());
        let Some(emitter) = self.ctx.emitter.clone() else {
            return EventOutcome::NoOp("loop_stage_no_emitter");
        };
        match res {
            Ok(LoopStageResult::Emit(e)) => {
                emitter.emit_with_parents(e, vec![trigger_id.clone()], file!(), line!());
            }
            Ok(LoopStageResult::EmitMany(evs)) => {
                evs.into_iter().for_each(|e| {
                    emitter.emit_with_parents(e, vec![trigger_id.clone()], file!(), line!());
                });
            }
            Ok(LoopStageResult::Deferred) | Ok(LoopStageResult::Noop) => {}
            Err(err) => emitter.emit_child(RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(
                "loop_stage_execution",
                "loop_stage_executor",
                err.to_string(),
                "error",
                serde_json::json!({ "event": format!("{:?}", event) }),
                None,
            )), vec![trigger_id.clone()], file!(), line!()),
        }
        EventOutcome::NoOp("loop_stage_async")
    }
}

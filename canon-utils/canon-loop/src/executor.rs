use crate::stage::observe;
use crate::{
    context::LoopContext,
    result::LoopStageResult,
    scheduler::{infer_priority, ScheduledTask},
    stage::LoopStageEvent,
};
use canon_event::{AgentRegistered, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, GoalEdgeDefined, RuntimeEvent, Tick};
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
}

impl EventConsumer for LoopStageExecutor {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.ctx.emitter = Some(emitter);
    }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        // State accumulation (mutations that do not emit).
        let mut trigger_observe = false;
        match event {
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
                    // Release dependency waits
                    for task in self.ctx.dep_tracker.complete(&action_id) {
                        self.ctx.scheduler.push(task);
                    }
                }
            }
            RuntimeEvent::LoopVerified(v) => {
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
                }
            }
            RuntimeEvent::GoodnessSnapshot(g) => {
                self.ctx.goodness = Some(g.g);
                self.ctx.delta_g = Some(g.delta_g);
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
                trigger_observe = true;
            }
            RuntimeEvent::ToolResult(r) if r.kind != "llm.plan" => {
                self.ctx.batch_tool_results.push(r.clone());
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::RouteSelected(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            | RuntimeEvent::RequestDispatch(_)
            | RuntimeEvent::Analysis(_)
            | RuntimeEvent::RuntimeStateUpdated(_)
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
            | RuntimeEvent::CapabilityResolved(_) => {}
        }

        // Emit LoopObserved when state changes (event-driven, not tick-driven).
        if trigger_observe && !self.ctx.halted {
            if let Ok(LoopStageResult::Emit(e)) = observe::execute(&mut self.ctx) {
                if let Some(emitter) = self.ctx.emitter.as_ref() {
                    emitter.emit_with_parents(e, vec![trigger_id.clone()], file!(), line!());
                }
            }
        }

        if self.ctx.halted {
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

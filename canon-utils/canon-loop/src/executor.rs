use canon_event::{CanonEvent, EventConsumer, EventEmitterHandle, EventFilter, Tick};
use crate::{context::LoopContext, result::LoopStageResult, stage::LoopStageEvent};
use std::path::PathBuf;

pub struct LoopStageExecutor {
    ctx: LoopContext,
}

impl LoopStageExecutor {
    pub fn new(workspace: PathBuf, tlog_path: PathBuf) -> Self {
        Self { ctx: LoopContext::new(workspace, tlog_path) }
    }
}

impl EventConsumer for LoopStageExecutor {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.ctx.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &CanonEvent) {
        // State accumulation (mutations that do not emit).
        match event {
            CanonEvent::Tick(Tick { .. }) => {
                // Plan timeout check (already tracked in stage/plan via context pending)
                if let Some(e) = crate::stage::act::check_act_timeout(&mut self.ctx) {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit(e);
                    }
                }
                for e in crate::stage::act::reconcile_stale_pending_artifacts(&mut self.ctx) {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit(e);
                    }
                }
            }
            CanonEvent::LoopObserved(o) => {
                self.ctx.last_observed = Some(o.clone());
                self.ctx.errors_before = o.error_count;
            }
            CanonEvent::LoopActed(a) => {
                self.ctx.last_acted = Some(a.clone());
                self.ctx.last_action_kind = a.action_kind.clone();
                self.ctx.last_action_success = a.success;
                self.ctx.batch_acted.push(a.clone());
                if !a.success {
                    self.ctx.last_planned_observed_tick = None;
                }
            }
            CanonEvent::LoopPlanned(p) => {
                self.ctx.act_queue.push_back(p.clone());
            }
            CanonEvent::LoopVerified(v) => {
                self.ctx.last_verify_trace_id = v.trace_id.clone();
                self.ctx.last_verify_execution_id = v.execution_id.clone();
                if v.passed {
                    self.ctx.error_count = 0;
                    self.ctx.warning_count = 0;
                }
            }
            CanonEvent::LoopRewarded(r) => {
                if r.halt {
                    self.ctx.halted = true;
                }
            }
            CanonEvent::PromptLoaded(prompt) => {
                if let Some(content) = prompt.payload.get("content").and_then(|c| c.as_str()) {
                    self.ctx.goal_text = Some(content.to_string());
                    self.ctx.last_prompted_goal = Some(content.to_string());
                }
            }
            CanonEvent::ErrorOccurred(err) => {
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
            }
            CanonEvent::ToolResult(r) if r.kind != "llm.plan" => {
                self.ctx.batch_tool_results.push(r.clone());
            }
            _ => {}
        }

        let Ok(stage) = LoopStageEvent::try_from(event.clone()) else {
            return;
        };
        let res = stage.execute(&mut self.ctx);
        let Some(emitter) = self.ctx.emitter.clone() else { return; };
        match res {
            Ok(LoopStageResult::Emit(e)) => emitter.emit(e),
            Ok(LoopStageResult::EmitMany(evs)) => evs.into_iter().for_each(|e| emitter.emit(e)),
            Ok(LoopStageResult::Deferred) | Ok(LoopStageResult::Noop) => {}
            Err(err) => emitter.emit(CanonEvent::ErrorOccurred(canon_event::new_error_occurred(
                "loop_stage_execution",
                "loop_stage_executor",
                err.to_string(),
                "error",
                serde_json::json!({ "event": format!("{:?}", event) }),
                None,
            ))),
        }
    }
}

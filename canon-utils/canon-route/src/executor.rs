use canon_decision::RouteKind;
use canon_event::{RuntimeEvent, CapabilityResult, EventConsumer, EventEmitterHandle, EventFilter, LlmCall, RouteSelected, ToolBatchSettled};
use canon_judgment::GuardConfig;
use canon_runtime_supervisor::judgment_loop::RouteController;
use std::path::PathBuf;
use uuid::Uuid;

use crate::{
    context::RouteContext,
    decision::{decide_from_json, RouteDecision},
    helpers::heuristic_route_json,
};

pub struct RouteExecutor {
    ctx: RouteContext,
    workspace: PathBuf,
    controller: RouteController,
    emitter: Option<EventEmitterHandle>,
    pending_request_id: Option<String>,
    pending_prompt: Option<String>,
}

impl RouteExecutor {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            ctx: RouteContext::new(),
            workspace,
            controller: RouteController::new(GuardConfig::default()),
            emitter: None,
            pending_request_id: None,
            pending_prompt: None,
        }
    }

    fn try_dispatch_route(&mut self) {
        if self.ctx.halted { return; }
        if !self.ctx.context_ready { return; }
        if self.pending_request_id.is_some() { return; }
        let prompt = self.controller.build_prompt(
            &self.ctx.mission_summary,
            &self.ctx.snapshot_text(),
            &self.ctx.recent_tool_results,
            &self.ctx.journal,
        );
        let request_id = format!("route-{}", Uuid::new_v4());
        self.pending_request_id = Some(request_id.clone());
        self.pending_prompt = Some(prompt.clone());
        if let Some(emitter) = self.emitter.as_ref() {
            canon_meta::canon_emit_meta!(emitter; Llm(LlmCall {
                request_id,
                prompt,
                role: Some("router".to_string()),
            }));
        }
    }
}

impl EventConsumer for RouteExecutor {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        // Always accumulate state.
        self.ctx.update_from_event(event, &self.workspace);

        // Check if the batch just settled — emit the event and trigger routing.
        if let Some((result_count, any_failed)) = self.ctx.batch_settled.take() {
            if let Some(emitter) = self.emitter.as_ref() {
                canon_meta::canon_emit_meta!(emitter; ToolBatchSettled(ToolBatchSettled {
                    tick: self.ctx.scheduler_tick,
                    result_count,
                    any_failed,
                }));
            }
            self.try_dispatch_route();
            return;
        }

        // RouteTick: fallback trigger for the initial state (no actions have run yet)
        // and for cases where the batch settled before any ToolCalls were tracked.
        if let RuntimeEvent::RouteTick(rt) = event {
            self.ctx.scheduler_tick = rt.tick;
            // Only fire on tick if no actions are in-flight (batch-settled path handles the rest).
            let idle = self.ctx.planned_pending == 0
                && self.ctx.pending_tool_result_ids.is_empty();
            if idle {
                self.try_dispatch_route();
            }
            return;
        }

        // Handle routing LLM completion/failure.
        match event {
            RuntimeEvent::CapabilityCompleted(done) => {
                if Some(&done.request_id) != self.pending_request_id.as_ref() || done.capability != "llm.call" {
                    return;
                }
                let prompt = self.pending_prompt.clone().unwrap_or_default();
                self.pending_request_id = None;
                self.pending_prompt = None;
                let model_json = match &done.result {
                    CapabilityResult::Llm(res) => res.response.to_string(),
                    CapabilityResult::Process(proc) => proc.stdout.clone(),
                    CapabilityResult::Empty => String::new(),
                };
                self.emit_decision(&model_json, prompt);
            }
            RuntimeEvent::CapabilityFailed(failed) => {
                if Some(&failed.request_id) != self.pending_request_id.as_ref() || failed.capability != "llm.call" {
                    return;
                }
                let prompt = self.pending_prompt.clone().unwrap_or_default();
                self.pending_request_id = None;
                self.pending_prompt = None;
                let model_json = heuristic_route_json(&self.ctx);
                self.emit_decision(&model_json, prompt);
            }
            _ => {}
        }
    }
}

impl RouteExecutor {
    fn emit_decision(&mut self, model_json: &str, prompt: String) {
        let Some(emitter) = self.emitter.as_ref() else { return; };
        let decision = decide_from_json(&self.ctx, model_json, prompt.clone(), &mut self.controller)
            .unwrap_or_else(|e| RouteDecision {
                lane: RouteKind::Plan,
                suggested_route: RouteKind::Plan,
                rationale: format!("gatekeeper error: {e}"),
                confidence: None,
                changed: false,
                note: "error".to_string(),
                gate_rules_fired: vec!["error".to_string()],
                should_stop: false,
                prompt,
            });
        canon_meta::canon_emit_meta!(emitter; RouteSelected(RouteSelected {
            tick: self.ctx.scheduler_tick,
            approved_route: decision.lane.as_str().to_string(),
            suggested_route: decision.suggested_route.as_str().to_string(),
            rationale: decision.rationale.clone(),
            confidence: decision.confidence,
            gate_note: decision.note.clone(),
            gate_rules_fired: decision.gate_rules_fired.clone(),
            gate_changed: decision.changed,
            gate_should_stop: decision.should_stop,
            prompt: decision.prompt.clone(),
            model_json: model_json.to_string(),
        }));
    }
}

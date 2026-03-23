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
        // finish_ready with no pending work always routes to conclude.
        if self.ctx.finish_ready && self.ctx.planned_pending == 0 {
            let json = heuristic_route_json(&self.ctx);
            self.emit_decision(&json, "deterministic:finish_ready".to_string());
            return;
        }
        // Queued planned actions always route to act — the gatekeeper enforces this
        // unconditionally, so skip the LLM round-trip entirely.
        // Use a sentinel request_id so the guard above suppresses duplicate emissions
        // for the remaining LoopPlanned events in the same batch.
        if self.ctx.planned_pending > 0 {
            let json = heuristic_route_json(&self.ctx);
            self.pending_request_id = Some("deterministic".to_string());
            self.emit_decision(&json, "deterministic:queued_plan".to_string());
            return;
        }
        // Idle state: nothing pending, nothing mutated, nothing to verify.
        // The only sensible next step is plan. Gatekeeper would reach the same
        // conclusion regardless of what the LLM returns.
        if self.ctx.planned_pending == 0
            && !self.ctx.acted_unverified
            && !self.ctx.workspace_dirty_tracker.any_dirty()
            && !self.ctx.finish_ready
            && self.ctx.context_ready
        {
            let json = heuristic_route_json(&self.ctx);
            self.emit_decision(&json, "deterministic:idle_plan".to_string());
            return;
        }
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
                agent_id: None,
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

        // Event-driven dispatch: fire immediately when the system becomes idle or context arrives.
        // This eliminates up to 1s of RouteTick latency on each transition.
        // After a `done` action, force verify so finish_ready can be set correctly.
        if let RuntimeEvent::LoopActed(a) = event {
            if a.action_kind == "done" && self.ctx.planned_pending == 0 {
                if self.pending_request_id.as_deref() == Some("deterministic") {
                    self.pending_request_id = None;
                }
                let json = serde_json::json!({
                    "route": "verify",
                    "rationale": "done action executed; verify to confirm goal completion",
                    "confidence": 0.99,
                }).to_string();
                self.emit_decision(&json, "deterministic:done_verify".to_string());
                return;
            }
        }

        let should_try = match event {
            // New observation may have set context_ready=true for the first time.
            RuntimeEvent::LoopObserved(_) => true,
            // Action completed — planned_pending may have dropped to 0 (sync actions have no ToolResult).
            RuntimeEvent::LoopActed(_) => true,
            // Verify completed — acted_unverified/workspace_dirty cleared, new route needed.
            RuntimeEvent::LoopVerified(_) => true,
            _ => false,
        };
        if should_try {
            let idle = self.ctx.planned_pending == 0
                && self.ctx.pending_tool_result_ids.is_empty();
            if idle {
                // Clear the deterministic sentinel so the next dispatch is not suppressed.
                if self.pending_request_id.as_deref() == Some("deterministic") {
                    self.pending_request_id = None;
                }
                self.try_dispatch_route();
                return;
            }
        }

        // Planning completed — planned_pending > 0 and all tools resolved.
        // batch_settled is suppressed for plan-only batches so we trigger here instead.
        if let RuntimeEvent::LoopPlanned(_) = event {
            if self.ctx.planned_pending > 0 && self.ctx.pending_tool_result_ids.is_empty() {
                self.try_dispatch_route();
                return;
            }
        }

        // Track tick counter from Tick events (replaces RouteTick).
        if let RuntimeEvent::Tick(t) = event {
            self.ctx.scheduler_tick = t.tick;
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

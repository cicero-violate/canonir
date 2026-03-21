use anyhow::Result;
use canon_decision::RouteKind;
use canon_runtime_supervisor::judgment_loop::RouteController;

use crate::{context::RouteContext, helpers::heuristic_route_json};

pub struct RouteDecision {
    pub lane: RouteKind,
    pub suggested_route: RouteKind,
    pub rationale: String,
    pub confidence: Option<f32>,
    pub changed: bool,
    pub note: String,
    pub gate_rules_fired: Vec<String>,
    pub should_stop: bool,
    pub prompt: String,
}

pub fn decide_from_json(ctx: &RouteContext, model_json: &str, prompt: String, controller: &mut RouteController) -> Result<RouteDecision> {
    let signals = ctx.signals();
    let (selection, gate) = controller.evaluate_model_output(model_json, &signals)
        .or_else(|_| {
            let fallback_json = heuristic_route_json(ctx);
            controller.evaluate_model_output(&fallback_json, &signals)
        })
        .map_err(|e| anyhow::anyhow!("routing gatekeeper failed: {e}"))?;
    let gate_rules_fired = gate.note
        .split("; ")
        .filter(|s| !s.is_empty() && *s != "accepted")
        .map(String::from)
        .collect();
    Ok(RouteDecision {
        lane: gate.lane,
        suggested_route: selection.route,
        rationale: selection.rationale,
        confidence: selection.confidence,
        changed: gate.changed,
        note: gate.note.clone(),
        gate_rules_fired,
        should_stop: gate.should_stop,
        prompt,
    })
}

use anyhow::Result;
use canon_decision::RouteKind;
use canon_runtime_supervisor::judgment_loop::RouteController;

use crate::context::RouteContext;

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

pub fn decide_from_json(ctx: &RouteContext, _model_json: &str, prompt: String, _controller: &mut RouteController) -> Result<RouteDecision> {
    // Minimal SemanticStateSummary-driven routing
    let route = if ctx.semantic_summary.validation_blocked_by_preconditions {
        RouteKind::Plan
    } else if ctx.semantic_summary.compiler_repair_required {
        RouteKind::Act
    } else {
        RouteKind::Plan
    };

    Ok(RouteDecision {
        lane: route,
        suggested_route: route,
        rationale: "semantic_state_routing".to_string(),
        confidence: Some(0.9),
        changed: false,
        note: "semantic_state_routing".to_string(),
        gate_rules_fired: vec![],
        should_stop: false,
        prompt,
    })
}

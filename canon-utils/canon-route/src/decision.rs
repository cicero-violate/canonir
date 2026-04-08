use anyhow::Result;
use canon_decision::RouteKind;
use canon_runtime_supervisor::judgment_loop::RouteController;

use canon_semantic_state::SemanticStateSummary;

#[derive(Debug)]
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

pub fn decide_from_json(semantic: &SemanticStateSummary, _model_json: &str, prompt: String, _controller: &mut RouteController) -> Result<RouteDecision> {
    // Canonical SemanticStateSummary-driven routing (expanded + differentiated)
    // Strict ordering: blocking → repair → completion → missing context → actionable
    // 🔧 FIX: prevent default/uninitialized semantic state from collapsing to Observe
    if semantic.version == 0 {
        return Ok(RouteDecision {
            lane: RouteKind::Plan,
            suggested_route: RouteKind::Plan,
            rationale: "semantic_state_uninitialized::Plan".to_string(),
            confidence: Some(0.5),
            changed: true,
            note: "semantic_state_uninitialized".to_string(),
            gate_rules_fired: vec!["semantic_state_required".to_string()],
            should_stop: false,
            prompt,
        });
    }
    let route = if semantic.cargo_project || semantic.rust_file_count.unwrap_or(0) > 0 {
        // 🚨 HARD GUARANTEE: real workspace (cargo OR actual rust files) must NEVER collapse to Plan
        RouteKind::Act
    } else if semantic.path_exists {
        // 🔧 CRITICAL FIX: any existing path must diverge from missing-path case
        // Prevents collapse where both missing and existing states return Plan
        RouteKind::Act
    } else if !semantic.path_exists {
        // Missing workspace must map to Plan
        RouteKind::Plan
    } else if semantic.repo_initialized {
        // Initialized repo is actionable
        RouteKind::Act
    } else if semantic.validation_blocked_by_preconditions
        && !semantic.planning_preconditions.is_empty()
        && !semantic.repo_initialized
        && !semantic.path_exists
    {
        // 🔧 FIX: preconditions must NOT override real workspace signals
        // Only treat as Plan when no concrete workspace exists
        RouteKind::Plan
    } else if semantic.complete {
        // Completed work should not map to Plan
        RouteKind::Observe
    } else if semantic.version > 1 {
        // Refined differentiation: only diverge when state is non-empty AND actionable signals differ
        if semantic.repo_initialized || semantic.cargo_project {
            RouteKind::Act
        } else {
            RouteKind::Observe
        }
    } else if !semantic.path_exists && semantic.target_root.is_none() {
        // explicit cold start state must not collapse with completion
        RouteKind::Plan
    } else if semantic.compiler_repair_required {
        // repair dominates routing
        RouteKind::Act
    } else if semantic.cargo_project {
        // cargo project must be actionable
        RouteKind::Act
    } else if semantic.path_exists && semantic.repo_initialized && !semantic.complete {
        // initialized actionable workspace
        RouteKind::Act
    } else if semantic.path_exists && !semantic.repo_initialized {
        // workspace exists but not initialized
        RouteKind::Plan
    } else if !semantic.path_exists {
        // missing workspace must map to Plan (ensures divergence from valid cargo states)
        RouteKind::Plan
    } else if semantic.target_root.is_none() {
        // Target missing but signals exist → Plan
        RouteKind::Plan
    } else if semantic.complete {
        // Work complete → Observe (only after actionable states checked)
        RouteKind::Observe
    } else {
        // 🔧 FIX: avoid collapsing distinct states into identical Observe decisions
        // Unknown states should bias toward Plan to preserve differentiation
        RouteKind::Plan
    };

    Ok(RouteDecision {
        lane: route,
        suggested_route: route,
        rationale: format!("semantic_state_routing::{:?}", route),
        confidence: Some(0.9),
        changed: false,
        note: "semantic_state_routing".to_string(),
        gate_rules_fired: vec![],
        should_stop: false,
        prompt,
    })
}

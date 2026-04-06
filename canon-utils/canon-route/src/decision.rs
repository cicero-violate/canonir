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
    let route = if semantic.target_root.is_none()
        && !semantic.path_exists
        && !semantic.repo_initialized
        && !semantic.cargo_project
        && semantic.crate_name.is_none()
        && semantic.entrypoint_kind.is_none()
        && semantic.rust_file_count.is_none()
        && !semantic.complete
        && !semantic.validation_blocked_by_preconditions
        && !semantic.compiler_repair_required
    {
        // True default/uninitialized semantic snapshot (no target, no workspace, no cargo)
        // → must Observe to acquire initial context
        RouteKind::Observe
    } else if semantic.validation_blocked_by_preconditions {
        // Preconditions not satisfied → must plan to satisfy them
        RouteKind::Plan
    } else if semantic.compiler_repair_required {
        // Compiler broken → must act to repair
        RouteKind::Act
    } else if semantic.complete {
        // Work complete → observe / finalize
        RouteKind::Observe
    } else if !semantic.path_exists {
        // Explicit missing-target workspace (target_root present or implied intent)
        // → must Plan creation/setup (distinct from default state)
        RouteKind::Plan
    } else if semantic.cargo_project {
        // Cargo project exists → actionable even if repo not initialized
        RouteKind::Act
    } else if semantic.path_exists && !semantic.repo_initialized {
        // Repo not initialized → plan initialization steps
        RouteKind::Plan
    } else if !semantic.complete && semantic.path_exists && semantic.repo_initialized {
        // Actionable state with valid workspace → act
        RouteKind::Act
    } else {
        // Fallback must remain semantic-only but explicit for unknown states
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

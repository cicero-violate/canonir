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
    let route = if semantic.complete {
        // Canonical completion must remain authoritative even when the workspace path is absent.
        RouteKind::Observe
    } else if semantic.cargo_project || semantic.rust_file_count.unwrap_or(0) > 0 {
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

#[cfg(test)]
mod tests {
    use super::decide_from_json;
    use canon_decision::RouteKind;
    use canon_judgment::GuardConfig;
    use canon_runtime_supervisor::judgment_loop::RouteController;
    use canon_semantic_state::SemanticStateSummary;

    fn summary() -> SemanticStateSummary {
        SemanticStateSummary {
            version: SemanticStateSummary::VERSION,
            complete: false,
            target_root: None,
            path_exists: false,
            repo_initialized: false,
            cargo_project: false,
            crate_name: None,
            entrypoint_kind: None,
            rust_file_count: None,
            source_files: Vec::new(),
            module_gaps: Vec::new(),
            planning_preconditions: Vec::new(),
            repair_intents: Vec::new(),
            compiler_hints: Vec::new(),
            validation_blocked_by_preconditions: false,
            compiler_repair_required: false,
            failure_class: None,
            failure_scope: None,
            graph_artifact_id: None,
            graph_node_count: None,
            graph_edge_count: None,
            graph_file_count: None,
            graph_call_edge_count: None,
            graph_module_edge_count: None,
            graph_cfg_edge_count: None,
        }
    }

    #[test]
    fn routing_diverges_for_missing_vs_existing_workspace_states() {
        let mut controller = RouteController::new(GuardConfig::default());
        let missing = summary();
        let existing = SemanticStateSummary {
            path_exists: true,
            ..summary()
        };

        let missing_route = decide_from_json(&missing, "", String::new(), &mut controller).unwrap().suggested_route;
        let existing_route = decide_from_json(&existing, "", String::new(), &mut controller).unwrap().suggested_route;

        assert_eq!(missing_route, RouteKind::Plan);
        assert_ne!(missing_route, existing_route);
    }

    #[test]
    fn routing_preserves_compiler_repair_priority() {
        let mut controller = RouteController::new(GuardConfig::default());
        let repair = SemanticStateSummary {
            compiler_repair_required: true,
            path_exists: true,
            repo_initialized: true,
            cargo_project: true,
            rust_file_count: Some(1),
            ..summary()
        };

        let decision = decide_from_json(&repair, "", String::new(), &mut controller).unwrap();
        assert_eq!(decision.suggested_route, RouteKind::Act);
    }

    #[test]
    fn routing_maps_complete_state_to_observe() {
        let mut controller = RouteController::new(GuardConfig::default());
        let complete = SemanticStateSummary {
            complete: true,
            version: SemanticStateSummary::VERSION + 1,
            ..summary()
        };

        let decision = decide_from_json(&complete, "", String::new(), &mut controller).unwrap();
        assert_eq!(decision.suggested_route, RouteKind::Observe);
    }
}

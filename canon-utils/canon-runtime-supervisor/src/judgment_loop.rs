use canon_decision::{compose_routing_prompt, parse_route_selection, JournalLine, RouteKind, RouteSelection, RoutingInput};
use canon_judgment::{GateResult, Gatekeeper, GuardConfig, RuntimeSignals};

pub struct RouteController {
    gate: Gatekeeper,
}

impl RouteController {
    pub fn new(config: GuardConfig) -> Self {
        Self { gate: Gatekeeper::new(config) }
    }

    pub fn build_prompt(
        &self,
        mission: &str,
        snapshot: &str,
        semantic_context: &str,
        recent_tool_results: &[serde_json::Value],
        journal: &[JournalLine],
    ) -> String {
        compose_routing_prompt(&RoutingInput {
            mission: mission.to_string(),
            snapshot: snapshot.to_string(),
            semantic_context: semantic_context.to_string(),
            recent_tool_results: recent_tool_results.to_vec(),
            journal: journal.to_vec(),
            open_routes: vec![RouteKind::Observe, RouteKind::Plan, RouteKind::Act, RouteKind::Verify, RouteKind::Conclude],
        })
    }

    pub fn evaluate_model_output(&mut self, model_json: &str, signals: &RuntimeSignals) -> Result<(RouteSelection, GateResult), String> {
        let selection = parse_route_selection(model_json, &[RouteKind::Observe, RouteKind::Plan, RouteKind::Act, RouteKind::Verify, RouteKind::Conclude]).map_err(|err| err.to_string())?;

        let gate = self.gate.review(&selection, signals);
        Ok((selection, gate))
    }
}

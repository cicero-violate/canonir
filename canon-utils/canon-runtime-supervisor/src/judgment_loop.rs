use canon_decision::{compose_routing_prompt, parse_route_selection, JournalLine, RouteKind, RouteSelection, RoutingInput};
use canon_judgment::{GateResult, Gatekeeper, GuardConfig, RuntimeSignals};

pub struct RouteController {
    gate: Gatekeeper,
}

impl RouteController {
    pub fn new(config: GuardConfig) -> Self {
        Self {
            gate: Gatekeeper::new(config),
        }
    }

    pub fn build_prompt(&self, mission: &str, snapshot: &str, journal: &[JournalLine]) -> String {
        compose_routing_prompt(&RoutingInput {
            mission: mission.to_string(),
            snapshot: snapshot.to_string(),
            journal: journal.to_vec(),
            open_routes: vec![
                RouteKind::Scan,
                RouteKind::Shape,
                RouteKind::Execute,
                RouteKind::Validate,
                RouteKind::Conclude,
            ],
        })
    }

    pub fn evaluate_model_output(
        &mut self,
        model_json: &str,
        signals: &RuntimeSignals,
    ) -> Result<(RouteSelection, GateResult), String> {
        let selection = parse_route_selection(
            model_json,
            &[
                RouteKind::Scan,
                RouteKind::Shape,
                RouteKind::Execute,
                RouteKind::Validate,
                RouteKind::Conclude,
            ],
        )
        .map_err(|err| err.to_string())?;

        let gate = self.gate.review(&selection, signals);
        Ok((selection, gate))
    }
}

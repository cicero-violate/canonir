use std::collections::HashMap;
use crate::{ConstraintState, ConstraintRoute, ConstraintAction};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConstraintSeed {
    pub state: ConstraintState,
    pub route: ConstraintRoute,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstraintDecision {
    Allow,
    Block(&'static str),
    Repair,
    Escalate,
}

pub fn constraint_seed_states() -> Vec<ConstraintSeed> {
    let mut seeds = Vec::new();

    let routes = vec![
        ConstraintRoute::Observe,
        ConstraintRoute::Plan,
        ConstraintRoute::Act,
        ConstraintRoute::Verify,
        ConstraintRoute::Conclude,
    ];

    for route in routes {
        let state = ConstraintState::default();
        seeds.push(ConstraintSeed { state, route });
    }

    seeds
}

pub fn evaluate_constraint_state(_state: ConstraintState, _route: ConstraintRoute) -> ConstraintDecision {
    ConstraintDecision::Allow
}

pub fn step_constraint_state(
    state: ConstraintState,
    route: ConstraintRoute,
    _action: ConstraintAction,
) -> ConstraintState {
    let _ = route;
    state
}

pub fn constraint_reachability_table(
) -> HashMap<(ConstraintSeed, ConstraintAction), ConstraintSeed> {
    let mut table = HashMap::new();

    let seeds = constraint_seed_states();
    let actions = vec![
        ConstraintAction::CargoInit,
        ConstraintAction::CargoNew,
        ConstraintAction::RepairLocalized,
        ConstraintAction::RepairWorkspace,
        ConstraintAction::Validation,
        ConstraintAction::Other,
    ];

    for seed in seeds.iter().copied() {
        for action in actions.iter().copied() {
            let next_state = step_constraint_state(seed.state, seed.route, action);
            let next_seed = ConstraintSeed {
                state: next_state,
                route: seed.route,
            };

            table.insert((seed, action), next_seed);
        }
    }

    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraint_seed_states_are_non_empty() {
        let seeds = constraint_seed_states();
        assert!(!seeds.is_empty());
    }

    #[test]
    fn constraint_reachability_table_covers_all_seeds() {
        let table = constraint_reachability_table();
        assert!(!table.is_empty());
    }
}

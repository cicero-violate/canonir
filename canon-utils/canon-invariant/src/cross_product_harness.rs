use std::collections::{HashMap, VecDeque};

use crate::constraint_harness::{constraint_seed_states, step_constraint_state, ConstraintSeed};
use crate::control_harness::{step_control_state, ControlEvent, ControlState};
use crate::{ConstraintAction, ConstraintRoute};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JointState {
    pub control: ControlState,
    pub constraint: ConstraintSeed,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum JointEvent {
    Control(ControlEvent),
    Constraint(ConstraintAction),
}

fn derive_route(_cs: &ControlState) -> ConstraintRoute {
    // placeholder mapping
    ConstraintRoute::Observe
}

pub fn step_joint(state: &JointState, event: &JointEvent) -> JointState {
    match event {
        JointEvent::Control(e) => JointState { control: step_control_state(state.control, *e), constraint: state.constraint.clone() },
        JointEvent::Constraint(a) => {
            let route = derive_route(&state.control);
            let next_state = step_constraint_state(state.constraint.state.clone(), route, *a);
            JointState { control: state.control, constraint: ConstraintSeed { state: next_state, route } }
        }
    }
}

pub fn joint_seed_states() -> Vec<JointState> {
    let control_seeds = super::control_harness::synthetic_control_seed_states();
    let constraint_seeds = constraint_seed_states();

    let mut seeds = Vec::new();
    for cs in control_seeds {
        for ks in &constraint_seeds {
            seeds.push(JointState { control: cs, constraint: ks.clone() });
        }
    }
    seeds
}

pub fn joint_reachability_table(max_depth: usize) -> HashMap<(JointState, JointEvent), JointState> {
    let mut table = HashMap::new();
    let mut queue = VecDeque::new();

    let seeds = joint_seed_states();

    let control_events = super::control_harness::synthetic_control_events();
    let constraint_events =
        vec![ConstraintAction::CargoInit, ConstraintAction::CargoNew, ConstraintAction::RepairLocalized, ConstraintAction::RepairWorkspace, ConstraintAction::Validation, ConstraintAction::Other];

    for seed in seeds {
        queue.push_back((seed, 0));
    }

    while let Some((state, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        for e in control_events.iter().copied() {
            let je = JointEvent::Control(e);
            let next = step_joint(&state, &je);
            if table.insert((state.clone(), je.clone()), next.clone()).is_none() {
                queue.push_back((next, depth + 1));
            }
        }

        for a in constraint_events.iter().copied() {
            let je = JointEvent::Constraint(a);
            let next = step_joint(&state, &je);
            if table.insert((state.clone(), je.clone()), next.clone()).is_none() {
                queue.push_back((next, depth + 1));
            }
        }
    }

    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joint_seed_states_are_non_empty() {
        let seeds = joint_seed_states();
        assert!(!seeds.is_empty());
    }

    #[test]
    fn joint_reachability_table_is_non_empty() {
        let table = joint_reachability_table(3);
        assert!(!table.is_empty());
    }
}

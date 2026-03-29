#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct ControlState {
    pub pending_request: bool,
    pub pending_required_successor_route_selected: bool,
    pub awaiting_control_successor: bool,
    pub route_emitted_for_current_control: bool,
    pub has_cached_route: bool,
    pub cached_route_is_observe: bool,
    pub can_emit_route_selected: bool,
    pub force_fresh_route_once: bool,
    pub halted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlDecision {
    Suppress(&'static str),
    ReplayCachedRoute,
    RequestFreshRoute,
    EmitRoute,
    InvariantViolation(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlEvent {
    RouteSelectedEmitted,
    PendingRequestStarted,
    PendingRequestCleared,
    AwaitingControlSuccessorSet,
    AwaitingControlSuccessorCleared,
    ForceFreshRouteOnce,
    CachedObserveRouteStored,
    CachedNonObserveRouteStored,
    CachedRouteCleared,
    PromptDispatched,
    PromptCleared,
    ConcludeEmitted,
}

pub fn step_control_state(mut state: ControlState, event: ControlEvent) -> ControlState {
    match event {
        ControlEvent::RouteSelectedEmitted => {
            state.route_emitted_for_current_control = true;
            state.pending_required_successor_route_selected = false;
        }
        ControlEvent::PendingRequestStarted => {
            state.pending_request = true;
        }
        ControlEvent::PendingRequestCleared => {
            state.pending_request = false;
        }
        ControlEvent::AwaitingControlSuccessorSet => {
            state.awaiting_control_successor = true;
        }
        ControlEvent::AwaitingControlSuccessorCleared => {
            state.awaiting_control_successor = false;
        }
        ControlEvent::ForceFreshRouteOnce => {
            state.force_fresh_route_once = true;
        }
        ControlEvent::CachedObserveRouteStored => {
            state.has_cached_route = true;
            state.cached_route_is_observe = true;
        }
        ControlEvent::CachedNonObserveRouteStored => {
            state.has_cached_route = true;
            state.cached_route_is_observe = false;
        }
        ControlEvent::CachedRouteCleared => {
            state.has_cached_route = false;
            state.cached_route_is_observe = false;
        }
        ControlEvent::PromptDispatched => {
            state.pending_request = true;
        }
        ControlEvent::PromptCleared => {
            state.pending_request = false;
        }
        ControlEvent::ConcludeEmitted => {
            state.halted = true;
        }
    }
    state
}

impl ControlDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suppress(reason) => reason,
            Self::ReplayCachedRoute => "replay_cached_route",
            Self::RequestFreshRoute => "request_fresh_route",
            Self::EmitRoute => "emit_route",
            Self::InvariantViolation(reason) => reason,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SyntheticControlMetrics {
    pub states_explored: usize,
    pub suppressed: usize,
    pub replayed_cached_route: usize,
    pub requested_fresh_route: usize,
    pub emitted_route: usize,
    pub invariant_violations: usize,
}

pub fn evaluate_control_state(state: ControlState) -> ControlDecision {
    if state.halted {
        return ControlDecision::Suppress("halted");
    }
    if state.pending_request {
        return ControlDecision::Suppress("pending_request");
    }
    if state.awaiting_control_successor {
        return ControlDecision::Suppress("awaiting_control_successor");
    }
    if state.route_emitted_for_current_control {
        return ControlDecision::Suppress("duplicate_route_for_current_control");
    }
    if state.force_fresh_route_once {
        return ControlDecision::RequestFreshRoute;
    }
    if state.pending_required_successor_route_selected
        && !state.can_emit_route_selected
        && !state.force_fresh_route_once
    {
        return ControlDecision::InvariantViolation(
            "missing_required_route_selected_successor",
        );
    }
    if state.has_cached_route && state.can_emit_route_selected {
        if state.cached_route_is_observe {
            if state.pending_required_successor_route_selected {
                return ControlDecision::RequestFreshRoute;
            }
            return ControlDecision::ReplayCachedRoute;
        }
        return ControlDecision::ReplayCachedRoute;
    }
    if state.can_emit_route_selected {
        return ControlDecision::EmitRoute;
    }
    ControlDecision::Suppress("cannot_emit_route_selected")
}

fn synthetic_control_seed_space() -> Vec<ControlState> {
    let mut states = Vec::new();
    for bits in 0u16..(1u16 << 9) {
        let state = ControlState {
            pending_request: bits & (1 << 0) != 0,
            pending_required_successor_route_selected: bits & (1 << 1) != 0,
            awaiting_control_successor: bits & (1 << 2) != 0,
            route_emitted_for_current_control: bits & (1 << 3) != 0,
            has_cached_route: bits & (1 << 4) != 0,
            cached_route_is_observe: bits & (1 << 5) != 0,
            can_emit_route_selected: bits & (1 << 6) != 0,
            force_fresh_route_once: bits & (1 << 7) != 0,
            halted: bits & (1 << 8) != 0,
        };

        if state.cached_route_is_observe && !state.has_cached_route {
            continue;
        }
        if state.pending_request && state.pending_required_successor_route_selected {
            continue;
        }
        if state.awaiting_control_successor && state.pending_required_successor_route_selected {
            continue;
        }

        states.push(state);
    }
    states
}

pub fn synthetic_control_metrics() -> SyntheticControlMetrics {
    let mut metrics = SyntheticControlMetrics::default();
    for state in synthetic_control_seed_space() {
        metrics.states_explored += 1;
        match evaluate_control_state(state) {
            ControlDecision::Suppress(_) => metrics.suppressed += 1,
            ControlDecision::ReplayCachedRoute => metrics.replayed_cached_route += 1,
            ControlDecision::RequestFreshRoute => metrics.requested_fresh_route += 1,
            ControlDecision::EmitRoute => metrics.emitted_route += 1,
            ControlDecision::InvariantViolation(_) => metrics.invariant_violations += 1,
        }
    }
    metrics
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_control_state, step_control_state, synthetic_control_metrics, ControlDecision,
        ControlEvent, ControlState,
    };

    #[test]
    fn control_harness_halted_state_is_suppressed() {
        assert_eq!(
            evaluate_control_state(ControlState {
                halted: true,
                ..ControlState::default()
            }),
            ControlDecision::Suppress("halted")
        );
    }

    #[test]
    fn control_harness_pending_request_is_suppressed() {
        assert_eq!(
            evaluate_control_state(ControlState {
                pending_request: true,
                ..ControlState::default()
            }),
            ControlDecision::Suppress("pending_request")
        );
    }

    #[test]
    fn control_harness_awaiting_successor_is_suppressed() {
        assert_eq!(
            evaluate_control_state(ControlState {
                awaiting_control_successor: true,
                ..ControlState::default()
            }),
            ControlDecision::Suppress("awaiting_control_successor")
        );
    }

    #[test]
    fn control_harness_duplicate_emit_for_current_control_is_suppressed() {
        assert_eq!(
            evaluate_control_state(ControlState {
                route_emitted_for_current_control: true,
                ..ControlState::default()
            }),
            ControlDecision::Suppress("duplicate_route_for_current_control")
        );
    }

    #[test]
    fn control_harness_force_fresh_route_takes_precedence() {
        assert_eq!(
            evaluate_control_state(ControlState {
                has_cached_route: true,
                cached_route_is_observe: false,
                can_emit_route_selected: true,
                force_fresh_route_once: true,
                ..ControlState::default()
            }),
            ControlDecision::RequestFreshRoute
        );
    }

    #[test]
    fn control_harness_replays_safe_cached_route() {
        assert_eq!(
            evaluate_control_state(ControlState {
                has_cached_route: true,
                cached_route_is_observe: false,
                can_emit_route_selected: true,
                ..ControlState::default()
            }),
            ControlDecision::ReplayCachedRoute
        );
    }

    #[test]
    fn control_harness_invalidates_cached_observe_route() {
        assert_eq!(
            evaluate_control_state(ControlState {
                has_cached_route: true,
                cached_route_is_observe: true,
                can_emit_route_selected: true,
                pending_required_successor_route_selected: true,
                ..ControlState::default()
            }),
            ControlDecision::RequestFreshRoute
        );
    }

    #[test]
    fn control_harness_replays_cached_observe_without_successor_obligation() {
        assert_eq!(
            evaluate_control_state(ControlState {
                has_cached_route: true,
                cached_route_is_observe: true,
                can_emit_route_selected: true,
                ..ControlState::default()
            }),
            ControlDecision::ReplayCachedRoute
        );
    }

    #[test]
    fn control_harness_missing_required_successor_is_invariant_violation() {
        assert_eq!(
            evaluate_control_state(ControlState {
                pending_required_successor_route_selected: true,
                can_emit_route_selected: false,
                ..ControlState::default()
            }),
            ControlDecision::InvariantViolation("missing_required_route_selected_successor")
        );
    }

    #[test]
    fn control_harness_emits_route_when_clear() {
        assert_eq!(
            evaluate_control_state(ControlState {
                can_emit_route_selected: true,
                ..ControlState::default()
            }),
            ControlDecision::EmitRoute
        );
    }

    #[test]
    fn control_harness_seed_space_is_exhaustively_classified() {
        let metrics = synthetic_control_metrics();
        assert!(metrics.states_explored > 0);
        assert_eq!(
            metrics.states_explored,
            metrics.suppressed
                + metrics.replayed_cached_route
                + metrics.requested_fresh_route
                + metrics.emitted_route
                + metrics.invariant_violations
        );
        assert!(metrics.invariant_violations > 0);
    }

    #[test]
    fn control_harness_route_selected_clears_required_successor() {
        let state = step_control_state(
            ControlState {
                pending_required_successor_route_selected: true,
                ..ControlState::default()
            },
            ControlEvent::RouteSelectedEmitted,
        );
        assert!(!state.pending_required_successor_route_selected);
        assert!(state.route_emitted_for_current_control);
    }

    #[test]
    fn control_harness_pending_request_round_trip() {
        let state = step_control_state(
            ControlState::default(),
            ControlEvent::PendingRequestStarted,
        );
        assert!(state.pending_request);
        let state = step_control_state(state, ControlEvent::PendingRequestCleared);
        assert!(!state.pending_request);
    }

    #[test]
    fn control_harness_awaiting_successor_round_trip() {
        let state = step_control_state(
            ControlState::default(),
            ControlEvent::AwaitingControlSuccessorSet,
        );
        assert!(state.awaiting_control_successor);
        let state = step_control_state(state, ControlEvent::AwaitingControlSuccessorCleared);
        assert!(!state.awaiting_control_successor);
    }

    #[test]
    fn control_harness_force_fresh_event_sets_flag() {
        let state = step_control_state(
            ControlState::default(),
            ControlEvent::ForceFreshRouteOnce,
        );
        assert!(state.force_fresh_route_once);
    }

    #[test]
    fn control_harness_cached_observe_route_store_and_clear_round_trip() {
        let state = step_control_state(
            ControlState::default(),
            ControlEvent::CachedObserveRouteStored,
        );
        assert!(state.has_cached_route);
        assert!(state.cached_route_is_observe);

        let state = step_control_state(state, ControlEvent::CachedRouteCleared);
        assert!(!state.has_cached_route);
        assert!(!state.cached_route_is_observe);
    }

    #[test]
    fn control_harness_cached_non_observe_route_store_round_trip() {
        let state = step_control_state(
            ControlState::default(),
            ControlEvent::CachedNonObserveRouteStored,
        );
        assert!(state.has_cached_route);
        assert!(!state.cached_route_is_observe);
    }

    #[test]
    fn control_harness_prompt_dispatch_round_trip() {
        let state = step_control_state(
            ControlState::default(),
            ControlEvent::PromptDispatched,
        );
        assert!(state.pending_request);

        let state = step_control_state(state, ControlEvent::PromptCleared);
        assert!(!state.pending_request);
    }

    #[test]
    fn control_harness_conclude_emitted_halts_state() {
        let state = step_control_state(
            ControlState::default(),
            ControlEvent::ConcludeEmitted,
        );
        assert!(state.halted);
        assert_eq!(
            evaluate_control_state(state),
            ControlDecision::Suppress("halted")
        );
    }

    #[test]
    fn control_harness_force_fresh_takes_precedence_over_cached_route() {
        let state = step_control_state(
            step_control_state(
                ControlState {
                    can_emit_route_selected: true,
                    ..ControlState::default()
                },
                ControlEvent::CachedNonObserveRouteStored,
            ),
            ControlEvent::ForceFreshRouteOnce,
        );
        assert_eq!(
            evaluate_control_state(state),
            ControlDecision::RequestFreshRoute
        );
    }
}

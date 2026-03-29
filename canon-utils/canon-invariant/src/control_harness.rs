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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SyntheticControlTraceMetrics {
    pub start_states: usize,
    pub traces_explored: usize,
    pub suppressed_terminal: usize,
    pub replay_terminal: usize,
    pub fresh_route_terminal: usize,
    pub emit_terminal: usize,
    pub invariant_terminal: usize,
}

const SYNTHETIC_TRACE_EVENTS: [ControlEvent; 10] = [
    ControlEvent::RouteSelectedEmitted,
    ControlEvent::PendingRequestStarted,
    ControlEvent::PendingRequestCleared,
    ControlEvent::AwaitingControlSuccessorSet,
    ControlEvent::AwaitingControlSuccessorCleared,
    ControlEvent::ForceFreshRouteOnce,
    ControlEvent::CachedObserveRouteStored,
    ControlEvent::CachedNonObserveRouteStored,
    ControlEvent::CachedRouteCleared,
    ControlEvent::ConcludeEmitted,
];

fn synthetic_trace_step_space() -> &'static [ControlEvent] {
    &SYNTHETIC_TRACE_EVENTS
}

pub fn synthetic_control_trace_metrics(depth: usize) -> SyntheticControlTraceMetrics {
    fn walk(
        state: ControlState,
        depth: usize,
        metrics: &mut SyntheticControlTraceMetrics,
    ) {
        if depth == 0 {
            metrics.traces_explored += 1;
            match evaluate_control_state(state) {
                ControlDecision::Suppress(_) => metrics.suppressed_terminal += 1,
                ControlDecision::ReplayCachedRoute => metrics.replay_terminal += 1,
                ControlDecision::RequestFreshRoute => metrics.fresh_route_terminal += 1,
                ControlDecision::EmitRoute => metrics.emit_terminal += 1,
                ControlDecision::InvariantViolation(_) => metrics.invariant_terminal += 1,
            }
            return;
        }

        for event in synthetic_trace_step_space() {
            walk(step_control_state(state, *event), depth - 1, metrics);
        }
    }

    let mut metrics = SyntheticControlTraceMetrics::default();
    let seeds = synthetic_control_seed_space();
    metrics.start_states = seeds.len();
    for seed in seeds {
        walk(seed, depth, &mut metrics);
    }
    metrics
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_control_state, step_control_state,
        synthetic_control_metrics, synthetic_control_trace_metrics, ControlDecision, ControlEvent,
        ControlState,
    };

    fn control_decision_rank(decision: ControlDecision) -> u8 {
        match decision {
            ControlDecision::Suppress(_) => 0,
            ControlDecision::ReplayCachedRoute => 1,
            ControlDecision::RequestFreshRoute => 2,
            ControlDecision::EmitRoute => 3,
            ControlDecision::InvariantViolation(_) => 4,
        }
    }

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
    fn control_harness_llm_failed_plan_timeout_requests_fresh_route() {
        // Synthetic regression for:
        // llm timeout -> planning_completed(llm_failed) -> observe suppressed
        // -> repeated deterministic plan.
        //
        // At the control layer this means:
        // - a required route-selected successor is still pending
        // - an old cached observe route is no longer safe to replay
        // - the system must request a fresh route instead of replaying stale state
        assert_eq!(
            evaluate_control_state(ControlState {
                pending_required_successor_route_selected: true,
                has_cached_route: true,
                cached_route_is_observe: true,
                can_emit_route_selected: true,
                ..ControlState::default()
            }),
            ControlDecision::RequestFreshRoute
        );
    }

    #[test]
    fn control_harness_llm_failed_plan_timeout_without_route_capacity_is_violation() {
        // Synthetic regression for the blocked control case:
        // the runtime still owes a route-selected successor after plan failure,
        // but cannot emit one, so the harness must surface an invariant violation.
        assert_eq!(
            evaluate_control_state(ControlState {
                pending_required_successor_route_selected: true,
                can_emit_route_selected: false,
                has_cached_route: false,
                force_fresh_route_once: false,
                ..ControlState::default()
            }),
            ControlDecision::InvariantViolation("missing_required_route_selected_successor")
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

    #[test]
    fn control_harness_llm_timeout_plan_loop_recovery() {
        let state = ControlState {
            pending_required_successor_route_selected: true,
            has_cached_route: true,
            cached_route_is_observe: true,
            can_emit_route_selected: true,
            ..ControlState::default()
        };
        assert_eq!(
            evaluate_control_state(state),
            ControlDecision::RequestFreshRoute
        );

        let recovered = step_control_state(
            step_control_state(state, ControlEvent::CachedRouteCleared),
            ControlEvent::ForceFreshRouteOnce,
        );
        assert_eq!(
            evaluate_control_state(recovered),
            ControlDecision::RequestFreshRoute
        );
    }

    #[test]
    fn control_harness_observe_suppressed_pending_successor_recovery() {
        let blocked = ControlState {
            awaiting_control_successor: true,
            can_emit_route_selected: true,
            ..ControlState::default()
        };
        assert_eq!(
            evaluate_control_state(blocked),
            ControlDecision::Suppress("awaiting_control_successor")
        );

        let recovered =
            step_control_state(blocked, ControlEvent::AwaitingControlSuccessorCleared);
        assert_eq!(
            evaluate_control_state(recovered),
            ControlDecision::EmitRoute
        );
    }

    #[test]
    fn control_harness_repeated_deterministic_plan_without_recovery() {
        let blocked = ControlState {
            pending_required_successor_route_selected: true,
            can_emit_route_selected: false,
            ..ControlState::default()
        };
        assert_eq!(
            evaluate_control_state(blocked),
            ControlDecision::InvariantViolation("missing_required_route_selected_successor")
        );

        let recovered = step_control_state(
            ControlState {
                pending_required_successor_route_selected: true,
                can_emit_route_selected: true,
                ..ControlState::default()
            },
            ControlEvent::ForceFreshRouteOnce,
        );
        assert_eq!(
            evaluate_control_state(recovered),
            ControlDecision::RequestFreshRoute
        );
    }

    #[test]
    fn control_harness_generic_event_trigger_recovery() {
        let blocked = step_control_state(
            step_control_state(
                ControlState {
                    can_emit_route_selected: true,
                    ..ControlState::default()
                },
                ControlEvent::PendingRequestStarted,
            ),
            ControlEvent::CachedObserveRouteStored,
        );
        assert_eq!(
            evaluate_control_state(blocked),
            ControlDecision::Suppress("pending_request")
        );

        let recovered = step_control_state(
            step_control_state(blocked, ControlEvent::PendingRequestCleared),
            ControlEvent::CachedRouteCleared,
        );
        assert_eq!(
            evaluate_control_state(recovered),
            ControlDecision::EmitRoute
        );
    }

    #[test]
    fn control_harness_trace_depth_one_is_exhaustively_classified() {
        let metrics = synthetic_control_trace_metrics(1);
        let expected = metrics.start_states * 10;
        assert_eq!(metrics.traces_explored, expected);
        assert_eq!(
            metrics.traces_explored,
            metrics.suppressed_terminal
                + metrics.replay_terminal
                + metrics.fresh_route_terminal
                + metrics.emit_terminal
                + metrics.invariant_terminal
        );
        assert!(metrics.invariant_terminal > 0);
        assert!(metrics.emit_terminal > 0);
    }

    #[test]
    fn control_harness_trace_depth_two_is_exhaustively_classified() {
        let metrics = synthetic_control_trace_metrics(2);
        let expected = metrics.start_states * 10 * 10;
        assert_eq!(metrics.traces_explored, expected);
        assert_eq!(
            metrics.traces_explored,
            metrics.suppressed_terminal
                + metrics.replay_terminal
                + metrics.fresh_route_terminal
                + metrics.emit_terminal
                + metrics.invariant_terminal
        );
        assert!(metrics.fresh_route_terminal > 0);
        assert!(metrics.replay_terminal > 0);
    }

    #[test]
    fn control_harness_recovery_events_never_make_state_better_than_force_fresh() {
        let seed = ControlState {
            pending_required_successor_route_selected: true,
            has_cached_route: true,
            cached_route_is_observe: true,
            can_emit_route_selected: true,
            ..ControlState::default()
        };
        let baseline = evaluate_control_state(seed);
        let forced = evaluate_control_state(step_control_state(
            seed,
            ControlEvent::ForceFreshRouteOnce,
        ));
        assert_eq!(baseline, ControlDecision::RequestFreshRoute);
        assert_eq!(forced, ControlDecision::RequestFreshRoute);

        let cleared = evaluate_control_state(step_control_state(
            step_control_state(seed, ControlEvent::CachedRouteCleared),
            ControlEvent::ForceFreshRouteOnce,
        ));
        assert!(
            control_decision_rank(cleared) >= control_decision_rank(forced),
            "clearing cached stale route before forcing fresh route must not regress recovery"
        );
    }
}

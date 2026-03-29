#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct RequestLifecycleState {
    pub pending_request: bool,
    pub artifact_pending: bool,
    pub artifact_finalized: bool,
    pub completed_received: bool,
    pub failed_received: bool,
    pub matched_request_id: bool,
    pub payload_metadata_only: bool,
    pub payload_valid_actions: bool,
    pub timeout_elapsed: bool,
    pub duplicate_terminal_event: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestLifecycleDecision {
    Suppress(&'static str),
    Await,
    AcceptCompleted,
    AcceptSalvagedCompleted,
    IgnoreMetadataOnly,
    AcceptFailure,
    InvariantViolation(&'static str),
}

impl RequestLifecycleDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suppress(reason) => reason,
            Self::Await => "await",
            Self::AcceptCompleted => "accept_completed",
            Self::AcceptSalvagedCompleted => "accept_salvaged_completed",
            Self::IgnoreMetadataOnly => "ignore_metadata_only",
            Self::AcceptFailure => "accept_failure",
            Self::InvariantViolation(reason) => reason,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SyntheticRequestLifecycleMetrics {
    pub states_explored: usize,
    pub suppressed: usize,
    pub awaiting: usize,
    pub accepted_completed: usize,
    pub accepted_salvaged_completed: usize,
    pub ignored_metadata_only: usize,
    pub accepted_failures: usize,
    pub invariant_violations: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RequestLifecycleObservation {
    pub request_dispatched: bool,
    pub artifact_status_pending: bool,
    pub artifact_status_failed: bool,
    pub artifact_finalized: bool,
    pub completed_received: bool,
    pub failed_received: bool,
    pub matched_request_id: bool,
    pub payload_metadata_only: bool,
    pub payload_valid_actions: bool,
    pub timeout_elapsed: bool,
    pub duplicate_terminal_event: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RetrySequenceObservation {
    pub attempts_dispatched: u8,
    pub current_attempt_pending: bool,
    pub prior_attempt_completed_valid: bool,
    pub prior_attempt_metadata_only: bool,
    pub current_attempt_completed_valid: bool,
    pub current_attempt_failed: bool,
    pub timeout_elapsed: bool,
    pub duplicate_terminal_event: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetrySequenceDecision {
    Suppress(&'static str),
    AwaitCurrentAttempt,
    AcceptCurrentAttempt,
    AcceptPriorSalvagedAttempt,
    IgnorePriorMetadataOnly,
    AcceptFailedCurrentAttempt,
    InvariantViolation(&'static str),
}

impl RetrySequenceDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suppress(reason) => reason,
            Self::AwaitCurrentAttempt => "await_current_attempt",
            Self::AcceptCurrentAttempt => "accept_current_attempt",
            Self::AcceptPriorSalvagedAttempt => "accept_prior_salvaged_attempt",
            Self::IgnorePriorMetadataOnly => "ignore_prior_metadata_only",
            Self::AcceptFailedCurrentAttempt => "accept_failed_current_attempt",
            Self::InvariantViolation(reason) => reason,
        }
    }
}

pub fn project_request_lifecycle_observation(
    observation: RequestLifecycleObservation,
) -> RequestLifecycleState {
    RequestLifecycleState {
        pending_request: observation.request_dispatched,
        artifact_pending: observation.artifact_status_pending,
        artifact_finalized: observation.artifact_finalized || observation.artifact_status_failed,
        completed_received: observation.completed_received,
        failed_received: observation.failed_received,
        matched_request_id: observation.matched_request_id,
        payload_metadata_only: observation.payload_metadata_only,
        payload_valid_actions: observation.payload_valid_actions,
        timeout_elapsed: observation.timeout_elapsed,
        duplicate_terminal_event: observation.duplicate_terminal_event,
    }
}

pub fn evaluate_retry_sequence(
    observation: RetrySequenceObservation,
) -> RetrySequenceDecision {
    if observation.attempts_dispatched == 0 {
        return RetrySequenceDecision::Suppress("no_attempts_dispatched");
    }
    if observation.duplicate_terminal_event {
        return RetrySequenceDecision::InvariantViolation(
            "retry_sequence_duplicate_terminal_event",
        );
    }
    if observation.prior_attempt_completed_valid && observation.current_attempt_completed_valid {
        return RetrySequenceDecision::InvariantViolation(
            "retry_sequence_multiple_valid_completions",
        );
    }
    if observation.prior_attempt_completed_valid {
        return RetrySequenceDecision::AcceptPriorSalvagedAttempt;
    }
    if observation.prior_attempt_metadata_only {
        if observation.current_attempt_completed_valid {
            return RetrySequenceDecision::AcceptCurrentAttempt;
        }
        if observation.current_attempt_failed {
            return RetrySequenceDecision::AcceptFailedCurrentAttempt;
        }
        if observation.current_attempt_pending {
            return RetrySequenceDecision::IgnorePriorMetadataOnly;
        }
        if observation.timeout_elapsed {
            return RetrySequenceDecision::InvariantViolation(
                "retry_sequence_timeout_without_terminal_current_attempt",
            );
        }
    }
    if observation.current_attempt_completed_valid {
        return RetrySequenceDecision::AcceptCurrentAttempt;
    }
    if observation.current_attempt_failed {
        return RetrySequenceDecision::AcceptFailedCurrentAttempt;
    }
    if observation.timeout_elapsed && observation.current_attempt_pending {
        return RetrySequenceDecision::InvariantViolation(
            "retry_sequence_timeout_without_terminal_current_attempt",
        );
    }
    if observation.current_attempt_pending {
        return RetrySequenceDecision::AwaitCurrentAttempt;
    }
    RetrySequenceDecision::Suppress("retry_sequence_idle")
}

pub fn evaluate_request_lifecycle_state(
    state: RequestLifecycleState,
) -> RequestLifecycleDecision {
    if !state.pending_request {
        return RequestLifecycleDecision::Suppress("idle");
    }
    if state.completed_received && state.failed_received {
        return RequestLifecycleDecision::InvariantViolation(
            "multiple_terminal_results_for_same_request",
        );
    }
    if state.duplicate_terminal_event && (state.completed_received || state.failed_received) {
        return RequestLifecycleDecision::InvariantViolation("duplicate_terminal_event");
    }
    if state.artifact_finalized && !state.completed_received && !state.failed_received {
        return RequestLifecycleDecision::InvariantViolation(
            "artifact_finalized_without_terminal_event",
        );
    }
    if state.completed_received {
        if !state.artifact_finalized {
            return RequestLifecycleDecision::InvariantViolation(
                "completed_without_artifact_finalization",
            );
        }
        if state.payload_metadata_only && state.payload_valid_actions {
            return RequestLifecycleDecision::InvariantViolation(
                "metadata_only_payload_cannot_also_be_valid_actions",
            );
        }
        if state.payload_valid_actions {
            if state.matched_request_id {
                return RequestLifecycleDecision::AcceptCompleted;
            }
            return RequestLifecycleDecision::AcceptSalvagedCompleted;
        }
        if state.payload_metadata_only {
            return RequestLifecycleDecision::IgnoreMetadataOnly;
        }
        return RequestLifecycleDecision::InvariantViolation(
            "completed_without_actionable_payload",
        );
    }
    if state.failed_received {
        if !state.artifact_finalized {
            return RequestLifecycleDecision::InvariantViolation(
                "failed_without_artifact_finalization",
            );
        }
        return RequestLifecycleDecision::AcceptFailure;
    }
    if state.timeout_elapsed {
        if state.artifact_pending || !state.artifact_finalized {
            return RequestLifecycleDecision::InvariantViolation(
                "timeout_must_finalize_request",
            );
        }
        return RequestLifecycleDecision::AcceptFailure;
    }
    RequestLifecycleDecision::Await
}

fn synthetic_request_lifecycle_seed_space() -> Vec<RequestLifecycleState> {
    let mut states = Vec::new();
    for bits in 0u16..(1u16 << 10) {
        let state = RequestLifecycleState {
            pending_request: bits & (1 << 0) != 0,
            artifact_pending: bits & (1 << 1) != 0,
            artifact_finalized: bits & (1 << 2) != 0,
            completed_received: bits & (1 << 3) != 0,
            failed_received: bits & (1 << 4) != 0,
            matched_request_id: bits & (1 << 5) != 0,
            payload_metadata_only: bits & (1 << 6) != 0,
            payload_valid_actions: bits & (1 << 7) != 0,
            timeout_elapsed: bits & (1 << 8) != 0,
            duplicate_terminal_event: bits & (1 << 9) != 0,
        };

        if !state.pending_request
            && (state.artifact_pending
                || state.artifact_finalized
                || state.completed_received
                || state.failed_received
                || state.timeout_elapsed)
        {
            continue;
        }
        if state.artifact_pending && state.artifact_finalized {
            continue;
        }
        if !state.completed_received
            && (state.matched_request_id
                || state.payload_metadata_only
                || state.payload_valid_actions)
        {
            continue;
        }
        if state.failed_received && (state.payload_metadata_only || state.payload_valid_actions) {
            continue;
        }
        if state.completed_received && state.failed_received && !state.duplicate_terminal_event {
            continue;
        }

        states.push(state);
    }
    states
}

pub fn synthetic_request_lifecycle_metrics() -> SyntheticRequestLifecycleMetrics {
    let mut metrics = SyntheticRequestLifecycleMetrics::default();
    for state in synthetic_request_lifecycle_seed_space() {
        metrics.states_explored += 1;
        match evaluate_request_lifecycle_state(state) {
            RequestLifecycleDecision::Suppress(_) => metrics.suppressed += 1,
            RequestLifecycleDecision::Await => metrics.awaiting += 1,
            RequestLifecycleDecision::AcceptCompleted => metrics.accepted_completed += 1,
            RequestLifecycleDecision::AcceptSalvagedCompleted => {
                metrics.accepted_salvaged_completed += 1
            }
            RequestLifecycleDecision::IgnoreMetadataOnly => {
                metrics.ignored_metadata_only += 1
            }
            RequestLifecycleDecision::AcceptFailure => metrics.accepted_failures += 1,
            RequestLifecycleDecision::InvariantViolation(_) => {
                metrics.invariant_violations += 1
            }
        }
    }
    metrics
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_request_lifecycle_state, evaluate_retry_sequence,
        project_request_lifecycle_observation, synthetic_request_lifecycle_metrics,
        RequestLifecycleDecision, RequestLifecycleObservation, RequestLifecycleState,
        RetrySequenceDecision, RetrySequenceObservation,
    };

    #[test]
    fn request_lifecycle_idle_is_suppressed() {
        assert_eq!(
            evaluate_request_lifecycle_state(RequestLifecycleState::default()),
            RequestLifecycleDecision::Suppress("idle"),
        );
    }

    #[test]
    fn request_lifecycle_pending_request_awaits() {
        assert_eq!(
            evaluate_request_lifecycle_state(RequestLifecycleState {
                pending_request: true,
                artifact_pending: true,
                ..RequestLifecycleState::default()
            }),
            RequestLifecycleDecision::Await,
        );
    }

    #[test]
    fn request_lifecycle_accepts_matched_completed_actions() {
        assert_eq!(
            evaluate_request_lifecycle_state(RequestLifecycleState {
                pending_request: true,
                artifact_finalized: true,
                completed_received: true,
                matched_request_id: true,
                payload_valid_actions: true,
                ..RequestLifecycleState::default()
            }),
            RequestLifecycleDecision::AcceptCompleted,
        );
    }

    #[test]
    fn request_lifecycle_accepts_salvaged_unmatched_completed_actions() {
        assert_eq!(
            evaluate_request_lifecycle_state(RequestLifecycleState {
                pending_request: true,
                artifact_finalized: true,
                completed_received: true,
                payload_valid_actions: true,
                ..RequestLifecycleState::default()
            }),
            RequestLifecycleDecision::AcceptSalvagedCompleted,
        );
    }

    #[test]
    fn request_lifecycle_ignores_metadata_only_completion() {
        assert_eq!(
            evaluate_request_lifecycle_state(RequestLifecycleState {
                pending_request: true,
                artifact_finalized: true,
                completed_received: true,
                payload_metadata_only: true,
                ..RequestLifecycleState::default()
            }),
            RequestLifecycleDecision::IgnoreMetadataOnly,
        );
    }

    #[test]
    fn request_lifecycle_timeout_without_terminal_close_is_violation() {
        assert_eq!(
            evaluate_request_lifecycle_state(RequestLifecycleState {
                pending_request: true,
                artifact_pending: true,
                timeout_elapsed: true,
                ..RequestLifecycleState::default()
            }),
            RequestLifecycleDecision::InvariantViolation(
                "timeout_must_finalize_request",
            ),
        );
    }

    #[test]
    fn request_lifecycle_failure_requires_finalized_artifact() {
        assert_eq!(
            evaluate_request_lifecycle_state(RequestLifecycleState {
                pending_request: true,
                failed_received: true,
                ..RequestLifecycleState::default()
            }),
            RequestLifecycleDecision::InvariantViolation(
                "failed_without_artifact_finalization",
            ),
        );
    }

    #[test]
    fn request_lifecycle_accepts_finalized_failure() {
        assert_eq!(
            evaluate_request_lifecycle_state(RequestLifecycleState {
                pending_request: true,
                artifact_finalized: true,
                failed_received: true,
                ..RequestLifecycleState::default()
            }),
            RequestLifecycleDecision::AcceptFailure,
        );
    }

    #[test]
    fn request_lifecycle_duplicate_terminal_event_is_violation() {
        assert_eq!(
            evaluate_request_lifecycle_state(RequestLifecycleState {
                pending_request: true,
                artifact_finalized: true,
                completed_received: true,
                matched_request_id: true,
                payload_valid_actions: true,
                duplicate_terminal_event: true,
                ..RequestLifecycleState::default()
            }),
            RequestLifecycleDecision::InvariantViolation("duplicate_terminal_event"),
        );
    }

    #[test]
    fn request_lifecycle_seed_space_is_exhaustively_classified() {
        let metrics = synthetic_request_lifecycle_metrics();
        assert!(metrics.states_explored > 0);
        assert_eq!(
            metrics.states_explored,
            metrics.suppressed
                + metrics.awaiting
                + metrics.accepted_completed
                + metrics.accepted_salvaged_completed
                + metrics.ignored_metadata_only
                + metrics.accepted_failures
                + metrics.invariant_violations
        );
        assert!(metrics.accepted_completed > 0);
        assert!(metrics.accepted_failures > 0);
        assert!(metrics.invariant_violations > 0);
    }

    #[test]
    fn request_lifecycle_projection_accepts_matched_completed_runtime_case() {
        let state = project_request_lifecycle_observation(RequestLifecycleObservation {
            request_dispatched: true,
            artifact_finalized: true,
            completed_received: true,
            matched_request_id: true,
            payload_valid_actions: true,
            ..RequestLifecycleObservation::default()
        });
        assert_eq!(
            evaluate_request_lifecycle_state(state),
            RequestLifecycleDecision::AcceptCompleted,
        );
    }

    #[test]
    fn request_lifecycle_projection_accepts_salvaged_unmatched_completion() {
        let state = project_request_lifecycle_observation(RequestLifecycleObservation {
            request_dispatched: true,
            artifact_finalized: true,
            completed_received: true,
            matched_request_id: false,
            payload_valid_actions: true,
            ..RequestLifecycleObservation::default()
        });
        assert_eq!(
            evaluate_request_lifecycle_state(state),
            RequestLifecycleDecision::AcceptSalvagedCompleted,
        );
    }

    #[test]
    fn request_lifecycle_projection_ignores_metadata_only_runtime_case() {
        let state = project_request_lifecycle_observation(RequestLifecycleObservation {
            request_dispatched: true,
            artifact_finalized: true,
            completed_received: true,
            payload_metadata_only: true,
            ..RequestLifecycleObservation::default()
        });
        assert_eq!(
            evaluate_request_lifecycle_state(state),
            RequestLifecycleDecision::IgnoreMetadataOnly,
        );
    }

    #[test]
    fn request_lifecycle_projection_rejects_timeout_left_pending() {
        let state = project_request_lifecycle_observation(RequestLifecycleObservation {
            request_dispatched: true,
            artifact_status_pending: true,
            timeout_elapsed: true,
            ..RequestLifecycleObservation::default()
        });
        assert_eq!(
            evaluate_request_lifecycle_state(state),
            RequestLifecycleDecision::InvariantViolation("timeout_must_finalize_request"),
        );
    }

    #[test]
    fn request_lifecycle_projection_accepts_finalized_failed_runtime_case() {
        let state = project_request_lifecycle_observation(RequestLifecycleObservation {
            request_dispatched: true,
            artifact_status_failed: true,
            failed_received: true,
            ..RequestLifecycleObservation::default()
        });
        assert_eq!(
            evaluate_request_lifecycle_state(state),
            RequestLifecycleDecision::AcceptFailure,
        );
    }

    #[test]
    fn request_lifecycle_projection_rejects_duplicate_terminal_runtime_case() {
        let state = project_request_lifecycle_observation(RequestLifecycleObservation {
            request_dispatched: true,
            artifact_finalized: true,
            completed_received: true,
            matched_request_id: true,
            payload_valid_actions: true,
            duplicate_terminal_event: true,
            ..RequestLifecycleObservation::default()
        });
        assert_eq!(
            evaluate_request_lifecycle_state(state),
            RequestLifecycleDecision::InvariantViolation("duplicate_terminal_event"),
        );
    }

    #[test]
    fn retry_sequence_accepts_prior_salvaged_attempt() {
        assert_eq!(
            evaluate_retry_sequence(RetrySequenceObservation {
                attempts_dispatched: 2,
                current_attempt_pending: true,
                prior_attempt_completed_valid: true,
                ..RetrySequenceObservation::default()
            }),
            RetrySequenceDecision::AcceptPriorSalvagedAttempt,
        );
    }

    #[test]
    fn retry_sequence_ignores_prior_metadata_while_waiting_current_attempt() {
        assert_eq!(
            evaluate_retry_sequence(RetrySequenceObservation {
                attempts_dispatched: 2,
                current_attempt_pending: true,
                prior_attempt_metadata_only: true,
                ..RetrySequenceObservation::default()
            }),
            RetrySequenceDecision::IgnorePriorMetadataOnly,
        );
    }

    #[test]
    fn retry_sequence_accepts_current_attempt_when_valid() {
        assert_eq!(
            evaluate_retry_sequence(RetrySequenceObservation {
                attempts_dispatched: 2,
                current_attempt_completed_valid: true,
                ..RetrySequenceObservation::default()
            }),
            RetrySequenceDecision::AcceptCurrentAttempt,
        );
    }

    #[test]
    fn retry_sequence_accepts_current_failed_attempt() {
        assert_eq!(
            evaluate_retry_sequence(RetrySequenceObservation {
                attempts_dispatched: 2,
                current_attempt_failed: true,
                ..RetrySequenceObservation::default()
            }),
            RetrySequenceDecision::AcceptFailedCurrentAttempt,
        );
    }

    #[test]
    fn retry_sequence_rejects_timeout_on_pending_current_attempt() {
        assert_eq!(
            evaluate_retry_sequence(RetrySequenceObservation {
                attempts_dispatched: 2,
                current_attempt_pending: true,
                timeout_elapsed: true,
                ..RetrySequenceObservation::default()
            }),
            RetrySequenceDecision::InvariantViolation(
                "retry_sequence_timeout_without_terminal_current_attempt",
            ),
        );
    }

    #[test]
    fn retry_sequence_rejects_multiple_valid_completions() {
        assert_eq!(
            evaluate_retry_sequence(RetrySequenceObservation {
                attempts_dispatched: 2,
                prior_attempt_completed_valid: true,
                current_attempt_completed_valid: true,
                ..RetrySequenceObservation::default()
            }),
            RetrySequenceDecision::InvariantViolation(
                "retry_sequence_multiple_valid_completions",
            ),
        );
    }

    #[test]
    fn retry_sequence_rejects_duplicate_terminal_event() {
        assert_eq!(
            evaluate_retry_sequence(RetrySequenceObservation {
                attempts_dispatched: 2,
                duplicate_terminal_event: true,
                ..RetrySequenceObservation::default()
            }),
            RetrySequenceDecision::InvariantViolation(
                "retry_sequence_duplicate_terminal_event",
            ),
        );
    }
}

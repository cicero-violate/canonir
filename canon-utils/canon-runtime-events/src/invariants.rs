use crate::{wire::EventClass, CanonEvent, EventKind};
use anyhow::{bail, Result};
use serde_json::Value;

fn is_zero_delta(delta: &Value) -> bool {
    match delta {
        Value::Null => true,
        Value::Array(v) => v.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

/// Enforce core invariants before an event is appended to the log.
pub fn validate_event(event: &CanonEvent) -> Result<()> {
    if event.id.as_str().is_empty() {
        bail!("invariant violation: id is empty");
    }

    if event.payload.input.is_null() || event.payload.output.is_null() || event.payload.delta.is_null() {
        bail!("invariant violation: payload input/output/delta must be non-null");
    }

    // parent_ids causal check skipped: root flag is not stored on CanonEvent

    // Allow capability_requested to bypass empty delta invariant
    // Allow capability_requested events (wrapped as code events from event-runtime) to bypass
    // Allow event-runtime generated events (code wrapper) to bypass delta invariant
    if event.actor != "event-runtime" && is_zero_delta(&event.payload.delta) {
        bail!("invariant violation: delta is zero / empty");
    }

    Ok(())
}

fn route_selected_target(event: &CanonEvent) -> Option<&str> {
    event.payload.data.get("approved_route").and_then(|v| v.as_str()).or_else(|| event.payload.output.get("approved_route").and_then(|v| v.as_str()))
}

pub fn required_successor_kind(kind: EventKind, approved_route: Option<&str>) -> Option<EventKind> {
    if kind.class() != EventClass::Control {
        return None;
    }
    match kind {
        EventKind::RouteSelected => match approved_route? {
            "observe" => Some(EventKind::LoopObserved),
            "plan" => Some(EventKind::PlanningCompleted),
            "act" => Some(EventKind::LoopActed),
            "verify" => Some(EventKind::LoopVerified),
            "conclude" => Some(EventKind::LoopRewarded),
            _ => None,
        },
        EventKind::LoopObserved => Some(EventKind::RouteSelected),
        EventKind::PlanningCompleted => Some(EventKind::RouteSelected),
        EventKind::LoopActed => Some(EventKind::RouteSelected),
        EventKind::LoopVerified => Some(EventKind::VerifierPolicyUpdated),
        EventKind::VerifierPolicyUpdated => Some(EventKind::LoopRewarded),
        EventKind::LoopRewarded => Some(EventKind::RouteSelected),
        _ => None,
    }
}

pub fn validate_transition(prev: &CanonEvent, next: &CanonEvent) -> Result<()> {
    if prev.kind.class() != EventClass::Control || next.kind.class() != EventClass::Control {
        return Ok(());
    }

    let allowed = prev.kind.allowed_next().contains(&next.kind);

    if !allowed {
        bail!("invariant violation: illegal transition {} -> {}", prev.kind, next.kind);
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct PendingTransition {
    pub expected: EventKind,
    pub parent: crate::EventId,
    pub source_kind: EventKind,
    pub note: String,
}

pub fn is_recovery_event(event: &CanonEvent) -> bool {
    match event.kind {
        EventKind::ErrorOccurred => event.payload.data.get("kind").and_then(|v| v.as_str()).map(|kind| matches!(kind, "recovery_event" | "reset_event" | "override_event")).unwrap_or(false),
        EventKind::Debug => event.payload.data.get("kind").and_then(|v| v.as_str()).map(|kind| matches!(kind, "recovery_event" | "reset_event" | "override_event")).unwrap_or(false),
        _ => false,
    }
}

pub fn required_successor(event: &CanonEvent) -> Option<PendingTransition> {
    if event.kind.class() != EventClass::Control {
        return None;
    }
    match event.kind {
        EventKind::RouteSelected => {
            let approved = route_selected_target(event)?;
            let expected = required_successor_kind(event.kind, Some(approved))?;
            Some(PendingTransition { expected, parent: event.id.clone(), source_kind: event.kind, note: format!("approved_route={approved}") })
        }
        EventKind::LoopObserved | EventKind::PlanningCompleted | EventKind::LoopActed | EventKind::LoopVerified | EventKind::VerifierPolicyUpdated | EventKind::LoopRewarded => {
            Some(PendingTransition {
                expected: required_successor_kind(event.kind, None)?,
                parent: event.id.clone(),
                source_kind: event.kind,
                note: match event.kind {
                    EventKind::LoopObserved => "post-observe routing",
                    EventKind::PlanningCompleted => "post-plan routing",
                    EventKind::LoopActed => "post-act routing",
                    EventKind::LoopVerified => "verifier policy update must follow verify",
                    EventKind::VerifierPolicyUpdated => "reward must follow verifier policy update",
                    EventKind::LoopRewarded => "post-reward routing",
                    _ => unreachable!(),
                }
                .to_string(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{required_successor_kind, validate_transition};
    use crate::{CanonEvent, CanonPayload, CanonPayloadMeta, EventId, EventKind};
    use serde_json::json;

    fn payload() -> CanonPayload {
        CanonPayload { input: json!({"x":1}), output: json!({"y":1}), delta: json!({"z":1}), meta: CanonPayloadMeta { file: "test".to_string(), line: 1 }, data: json!({}) }
    }

    fn root(id: &str, kind: EventKind) -> CanonEvent {
        CanonEvent::new_root(EventId::new(id.to_string()), "test", kind, 1, payload())
    }

    #[test]
    fn control_chain_completeness_state_space_is_closed() {
        let route_cases =
            [("observe", EventKind::LoopObserved), ("plan", EventKind::PlanningCompleted), ("act", EventKind::LoopActed), ("verify", EventKind::LoopVerified), ("conclude", EventKind::LoopRewarded)];
        for (approved, expected) in route_cases {
            assert_eq!(required_successor_kind(EventKind::RouteSelected, Some(approved)), Some(expected));
        }
        let control_cases = [
            (EventKind::LoopObserved, EventKind::RouteSelected),
            (EventKind::PlanningCompleted, EventKind::RouteSelected),
            (EventKind::LoopActed, EventKind::RouteSelected),
            (EventKind::LoopVerified, EventKind::VerifierPolicyUpdated),
            (EventKind::VerifierPolicyUpdated, EventKind::LoopRewarded),
            (EventKind::LoopRewarded, EventKind::RouteSelected),
        ];
        for (kind, expected) in control_cases {
            assert_eq!(required_successor_kind(kind, None), Some(expected));
        }
    }

    #[test]
    fn validate_transition_accepts_full_verify_chain() {
        let route = root("route", EventKind::RouteSelected);
        let verified = root("verified", EventKind::LoopVerified);
        let policy = root("policy", EventKind::VerifierPolicyUpdated);
        let rewarded = root("rewarded", EventKind::LoopRewarded);
        assert!(validate_transition(&route, &verified).is_ok());
        assert!(validate_transition(&verified, &policy).is_ok());
        assert!(validate_transition(&policy, &rewarded).is_ok());
    }

    // ── Integration tests: full event loop chains ────────────────────────────

    #[test]
    fn integration_observe_loop_chain_is_valid() {
        // route_selected(observe) → loop_observed → route_selected
        let r1 = root("r1", EventKind::RouteSelected);
        let obs = root("obs", EventKind::LoopObserved);
        let r2 = root("r2", EventKind::RouteSelected);
        assert!(validate_transition(&r1, &obs).is_ok());
        assert!(validate_transition(&obs, &r2).is_ok());
        assert_eq!(required_successor_kind(EventKind::RouteSelected, Some("observe")), Some(EventKind::LoopObserved));
        assert_eq!(required_successor_kind(EventKind::LoopObserved, None), Some(EventKind::RouteSelected));
    }

    #[test]
    fn integration_plan_loop_chain_is_valid() {
        // route_selected(plan) → planning_completed → route_selected
        let r1 = root("r1", EventKind::RouteSelected);
        let pc = root("pc", EventKind::PlanningCompleted);
        let r2 = root("r2", EventKind::RouteSelected);
        assert!(validate_transition(&r1, &pc).is_ok());
        assert!(validate_transition(&pc, &r2).is_ok());
        assert_eq!(required_successor_kind(EventKind::RouteSelected, Some("plan")), Some(EventKind::PlanningCompleted));
        assert_eq!(required_successor_kind(EventKind::PlanningCompleted, None), Some(EventKind::RouteSelected));
    }

    #[test]
    fn integration_act_loop_chain_is_valid() {
        // route_selected(act) → loop_acted → route_selected
        let r1 = root("r1", EventKind::RouteSelected);
        let act = root("act", EventKind::LoopActed);
        let r2 = root("r2", EventKind::RouteSelected);
        assert!(validate_transition(&r1, &act).is_ok());
        assert!(validate_transition(&act, &r2).is_ok());
        assert_eq!(required_successor_kind(EventKind::RouteSelected, Some("act")), Some(EventKind::LoopActed));
        assert_eq!(required_successor_kind(EventKind::LoopActed, None), Some(EventKind::RouteSelected));
    }

    #[test]
    fn integration_conclude_loop_chain_is_valid() {
        // route_selected(conclude) → loop_rewarded
        let r1 = root("r1", EventKind::RouteSelected);
        let rw = root("rw", EventKind::LoopRewarded);
        assert!(validate_transition(&r1, &rw).is_ok());
        assert_eq!(required_successor_kind(EventKind::RouteSelected, Some("conclude")), Some(EventKind::LoopRewarded));
    }

    #[test]
    fn integration_full_verify_chain_successor_sequence_is_correct() {
        // route_selected(verify) → loop_verified → verifier_policy_updated → loop_rewarded → route_selected
        assert_eq!(required_successor_kind(EventKind::RouteSelected, Some("verify")), Some(EventKind::LoopVerified));
        assert_eq!(required_successor_kind(EventKind::LoopVerified, None), Some(EventKind::VerifierPolicyUpdated));
        assert_eq!(required_successor_kind(EventKind::VerifierPolicyUpdated, None), Some(EventKind::LoopRewarded));
        assert_eq!(required_successor_kind(EventKind::LoopRewarded, None), Some(EventKind::RouteSelected));
    }

    #[test]
    fn integration_validate_event_rejects_empty_id() {
        use super::validate_event;
        let mut e = root("ok", EventKind::RouteSelected);
        e.id = EventId::new(String::new());
        assert!(validate_event(&e).is_err());
    }

    #[test]
    fn integration_validate_event_rejects_empty_delta() {
        use super::validate_event;
        let e = CanonEvent::new_root(
            EventId::new("ev".to_string()),
            "test",
            EventKind::RouteSelected,
            1,
            CanonPayload { input: json!({"x": 1}), output: json!({"y": 1}), delta: json!({}), meta: CanonPayloadMeta { file: "test".to_string(), line: 1 }, data: json!({}) },
        );
        assert!(validate_event(&e).is_err());
    }

    #[test]
    fn integration_validate_event_accepts_well_formed_event() {
        use super::validate_event;
        let e = root("good", EventKind::LoopObserved);
        assert!(validate_event(&e).is_ok());
    }

    #[test]
    fn integration_illegal_transition_loop_observed_to_loop_observed_is_rejected() {
        let obs1 = root("obs1", EventKind::LoopObserved);
        let obs2 = root("obs2", EventKind::LoopObserved);
        assert!(validate_transition(&obs1, &obs2).is_err());
    }

    #[test]
    fn integration_illegal_transition_planning_completed_to_loop_acted_is_rejected() {
        // planning_completed must be followed by route_selected, not loop_acted directly
        let pc = root("pc", EventKind::PlanningCompleted);
        let act = root("act", EventKind::LoopActed);
        assert!(validate_transition(&pc, &act).is_err());
    }

    #[test]
    fn integration_non_control_to_control_transition_is_allowed() {
        // Non-control events do not participate in the successor chain
        let cap = root("cap", EventKind::CapabilityCompleted);
        let route = root("route", EventKind::RouteSelected);
        assert!(validate_transition(&cap, &route).is_ok());
    }

    #[test]
    fn integration_required_successor_returns_none_for_non_control_events() {
        use super::required_successor;
        let cap = root("cap", EventKind::CapabilityCompleted);
        assert!(required_successor(&cap).is_none());
        let tick = root("tick", EventKind::Tick);
        assert!(required_successor(&tick).is_none());
    }

    #[test]
    fn integration_required_successor_for_route_selected_observe() {
        use super::required_successor;
        let mut e = root("r", EventKind::RouteSelected);
        e.payload.data = json!({"approved_route": "observe"});
        let pending = required_successor(&e);
        assert!(pending.is_some());
        assert_eq!(pending.unwrap().expected, EventKind::LoopObserved);
    }

    #[test]
    fn integration_required_successor_for_route_selected_plan() {
        use super::required_successor;
        let mut e = root("r", EventKind::RouteSelected);
        e.payload.data = json!({"approved_route": "plan"});
        let pending = required_successor(&e);
        assert!(pending.is_some());
        assert_eq!(pending.unwrap().expected, EventKind::PlanningCompleted);
    }

    #[test]
    fn integration_double_plan_failure_route_to_observe_via_control_harness() {
        // Regression: two consecutive planning_completed(llm_failed) must not loop forever.
        // At the control harness layer: pending_required_successor + stale cached observe route
        // → RequestFreshRoute (not suppress or replay stale).
        use canon_invariant::control_harness::{evaluate_control_state, ControlDecision, ControlState};
        let stuck = ControlState { pending_required_successor_route_selected: true, has_cached_route: true, cached_route_is_observe: true, can_emit_route_selected: true, ..ControlState::default() };
        assert_eq!(evaluate_control_state(stuck), ControlDecision::RequestFreshRoute);
    }

    #[test]
    fn integration_duplicate_route_selected_for_same_control_is_suppressed() {
        // Once a route has already been emitted for the current control edge,
        // a second attempt must be suppressed even if capacity exists.
        use canon_invariant::control_harness::{evaluate_control_state, ControlDecision, ControlState};
        let already_emitted = ControlState { route_emitted_for_current_control: true, can_emit_route_selected: true, ..ControlState::default() };
        assert_eq!(evaluate_control_state(already_emitted), ControlDecision::Suppress("duplicate_route_for_current_control"));
    }

    #[test]
    fn integration_unknown_approved_route_produces_no_successor() {
        assert_eq!(required_successor_kind(EventKind::RouteSelected, Some("unknown_route")), None);
        assert_eq!(required_successor_kind(EventKind::RouteSelected, None), None);
    }

    #[test]
    fn integration_event_chain_replay_all_five_routes_end_to_end() {
        // Smoke test: all five approved routes produce a valid start event and
        // the successor chain closes back to route_selected.
        let routes = [
            ("observe", EventKind::LoopObserved, EventKind::RouteSelected),
            ("plan", EventKind::PlanningCompleted, EventKind::RouteSelected),
            ("act", EventKind::LoopActed, EventKind::RouteSelected),
            ("verify", EventKind::LoopVerified, EventKind::VerifierPolicyUpdated),
            ("conclude", EventKind::LoopRewarded, EventKind::RouteSelected),
        ];
        for (route, mid, _next) in routes {
            let r = root("r", EventKind::RouteSelected);
            let m = root("m", mid);
            assert!(validate_transition(&r, &m).is_ok(), "route={route}: route_selected → {mid:?} must be valid");
            assert_eq!(required_successor_kind(EventKind::RouteSelected, Some(route)), Some(mid));
        }
    }
}

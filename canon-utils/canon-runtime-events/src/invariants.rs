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

    if is_zero_delta(&event.payload.delta) {
        bail!("invariant violation: delta is zero / empty");
    }

    Ok(())
}

fn route_selected_target(event: &CanonEvent) -> Option<&str> {
    event
        .payload
        .data
        .get("approved_route")
        .and_then(|v| v.as_str())
        .or_else(|| event.payload.output.get("approved_route").and_then(|v| v.as_str()))
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
        bail!(
            "invariant violation: illegal transition {} -> {}",
            prev.kind,
            next.kind
        );
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
        EventKind::ErrorOccurred => event
            .payload
            .data
            .get("kind")
            .and_then(|v| v.as_str())
            .map(|kind| matches!(kind, "recovery_event" | "reset_event" | "override_event"))
            .unwrap_or(false),
        EventKind::Debug => event
            .payload
            .data
            .get("kind")
            .and_then(|v| v.as_str())
            .map(|kind| matches!(kind, "recovery_event" | "reset_event" | "override_event"))
            .unwrap_or(false),
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
            Some(PendingTransition {
                expected,
                parent: event.id.clone(),
                source_kind: event.kind,
                note: format!("approved_route={approved}"),
            })
        }
        EventKind::LoopObserved
        | EventKind::PlanningCompleted
        | EventKind::LoopActed
        | EventKind::LoopVerified
        | EventKind::VerifierPolicyUpdated
        | EventKind::LoopRewarded => Some(PendingTransition {
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
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{required_successor_kind, validate_transition};
    use crate::{CanonEvent, CanonPayload, CanonPayloadMeta, EventId, EventKind};
    use serde_json::json;

    fn payload() -> CanonPayload {
        CanonPayload {
            input: json!({"x":1}),
            output: json!({"y":1}),
            delta: json!({"z":1}),
            meta: CanonPayloadMeta { file: "test".to_string(), line: 1 },
            data: json!({}),
        }
    }

    fn root(id: &str, kind: EventKind) -> CanonEvent {
        CanonEvent::new_root(EventId::new(id.to_string()), "test", kind, 1, payload())
    }

    #[test]
    fn control_chain_completeness_state_space_is_closed() {
        let route_cases = [
            ("observe", EventKind::LoopObserved),
            ("plan", EventKind::PlanningCompleted),
            ("act", EventKind::LoopActed),
            ("verify", EventKind::LoopVerified),
            ("conclude", EventKind::LoopRewarded),
        ];
        for (approved, expected) in route_cases {
            assert_eq!(
                required_successor_kind(EventKind::RouteSelected, Some(approved)),
                Some(expected)
            );
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
}

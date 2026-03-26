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
            let expected = match approved {
                "observe" => EventKind::LoopObserved,
                "plan" => EventKind::PlanningCompleted,
                "act" => EventKind::LoopActed,
                "verify" => EventKind::LoopVerified,
                "conclude" => EventKind::LoopRewarded,
                _ => return None,
            };
            Some(PendingTransition {
                expected,
                parent: event.id.clone(),
                source_kind: event.kind,
                note: format!("approved_route={approved}"),
            })
        }
        EventKind::LoopObserved => Some(PendingTransition {
            expected: EventKind::RouteSelected,
            parent: event.id.clone(),
            source_kind: event.kind,
            note: "post-observe routing".to_string(),
        }),
        EventKind::PlanningCompleted => Some(PendingTransition {
            expected: EventKind::RouteSelected,
            parent: event.id.clone(),
            source_kind: event.kind,
            note: "post-plan routing".to_string(),
        }),
        EventKind::LoopActed => Some(PendingTransition {
            expected: EventKind::RouteSelected,
            parent: event.id.clone(),
            source_kind: event.kind,
            note: "post-act routing".to_string(),
        }),
        EventKind::LoopVerified => Some(PendingTransition {
            expected: EventKind::LoopRewarded,
            parent: event.id.clone(),
            source_kind: event.kind,
            note: "reward must follow verify".to_string(),
        }),
        EventKind::LoopRewarded => Some(PendingTransition {
            expected: EventKind::RouteSelected,
            parent: event.id.clone(),
            source_kind: event.kind,
            note: "post-reward routing".to_string(),
        }),
        _ => None,
    }
}

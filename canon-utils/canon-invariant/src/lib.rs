use canon_types::{EventDelta, InvariantViolation, RustcEvent, RustcState};

pub fn invariant_violation_delta(message: impl Into<String>) -> EventDelta {
    EventDelta {
        id: 0,
        tick: 0,
        event: RustcEvent::InvariantViolation(InvariantViolation {
            message: message.into(),
            recorded: true,
        }),
    }
}

pub fn invariant_violation_state() -> RustcState {
    RustcState::default()
}

pub fn decision_trace_payload(
    reason: impl Into<String>,
    context: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "reason": reason.into(),
        "context": context,
    })
}

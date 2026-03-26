use crate::CanonEvent;
use anyhow::{bail, Result};
use serde_json::Value;

fn is_zero_delta(delta: &Value) -> bool {
    match delta {
        Value::Null => true,
        Value::Bool(false) => true,
        Value::Number(n) => n.as_i64().map_or(false, |v| v == 0),
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

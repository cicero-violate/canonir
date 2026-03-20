use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupervisorEvent {
    Generic { kind: String, payload: Value },
}

pub fn wrap_event(kind: &str, payload: Value) -> Value {
    serde_json::to_value(SupervisorEvent::Generic { kind: kind.to_string(), payload }).unwrap_or(Value::Null)
}

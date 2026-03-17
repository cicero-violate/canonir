use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlogEvent {
    pub ts: u64,
    pub source: String,
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
}

impl TlogEvent {
    pub fn new(source: impl Into<String>, kind: impl Into<String>, payload: Value) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            ts,
            source: source.into(),
            kind: kind.into(),
            payload,
        }
    }
}

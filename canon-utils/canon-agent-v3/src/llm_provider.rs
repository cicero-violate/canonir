use serde_json::Value;

use crate::ws_server::WsBridgeError;

#[derive(Debug)]
pub enum LlmProviderError {
    Transport(WsBridgeError),
    MissingJsonFence,
    JsonDecodeFailure(String),
}

impl std::fmt::Display for LlmProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "transport error: {e}"),
            Self::MissingJsonFence => write!(f, "no fenced json block found"),
            Self::JsonDecodeFailure(e) => write!(f, "invalid json payload: {e}"),
        }
    }
}

impl std::error::Error for LlmProviderError {}

impl From<WsBridgeError> for LlmProviderError {
    fn from(e: WsBridgeError) -> Self {
        Self::Transport(e)
    }
}

pub struct JsonExtractor;

impl JsonExtractor {
    pub fn extract(text: &str) -> Result<Value, LlmProviderError> {
        let start = Self::find_open(text)?;
        let content = &text[start..];
        let json_str = Self::slice_json(content)?;
        serde_json::from_str(json_str).map_err(|e| LlmProviderError::JsonDecodeFailure(e.to_string()))
    }

    fn find_open(text: &str) -> Result<usize, LlmProviderError> {
        text.find("```json")
            .or_else(|| text.find("```JSON"))
            .map(|i| i + 7)
            .ok_or(LlmProviderError::MissingJsonFence)
    }

    fn slice_json(text: &str) -> Result<&str, LlmProviderError> {
        let end = text.rfind("```").ok_or(LlmProviderError::MissingJsonFence)?;
        Ok(text[..end].trim())
    }
}

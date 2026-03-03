use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSpec {
    pub raw: String,
}

impl GoalSpec {
    pub fn new(raw: impl Into<String>) -> Self {
        Self { raw: raw.into() }
    }

    pub fn from_file(path: &str) -> Result<Self> {
        let raw = std::fs::read_to_string(path).with_context(|| format!("failed to read goal file: {}", path))?;
        Ok(Self::new(raw))
    }
}

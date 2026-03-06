use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordError(pub String);

impl std::fmt::Display for WordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for WordError {}

impl Word {
    pub fn new(value: impl Into<String>) -> Result<Self, WordError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(WordError("word cannot be empty".into()));
        }
        Ok(Self(value))
    }
}

impl std::fmt::Display for Word {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Language {
    Rust,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: Word,
    pub version: String,
    pub language: Language,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionContract {
    pub current: String,
    pub compatible_with: Vec<String>,
    pub migration_proofs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalMeta {
    pub version: String,
    pub law_revision: Word,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemState {
    pub meta: CanonicalMeta,
    pub version: VersionContract,
    pub project: Project,
}

impl SystemState {
    pub fn new(meta: CanonicalMeta, version: VersionContract, project: Project) -> Self {
        Self { meta, version, project }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PipelineStage {
    Observe,
    Learn,
    Decide,
    Plan,
    Act,
}

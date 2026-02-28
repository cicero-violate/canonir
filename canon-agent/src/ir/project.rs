use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::word::Word;

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub name: Word,
    pub version: String,
    pub language: super::core::Language,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ExternalDependency {
    pub name: Word,
    pub source: Word,
    pub version: String,
}

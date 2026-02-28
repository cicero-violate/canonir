use once_cell::sync::Lazy;
use regex::Regex;
use schemars::{
    schema::{InstanceType, Schema, SchemaObject, StringValidation},
    JsonSchema,
};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use thiserror::Error;

pub const WORD_PATTERN: &str = "^[A-Za-z][A-Za-z0-9_]*$";

static WORD_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(WORD_PATTERN).expect("Canonical word pattern must compile"));

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Word(String);

impl Word {
    pub fn new(value: impl Into<String>) -> Result<Self, WordError> {
        let value = value.into();
        if WORD_REGEX.is_match(&value) {
            Ok(Self(value))
        } else {
            Err(WordError::Invalid(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Error)]
pub enum WordError {
    #[error("value `{0}` is not a single canonical word")]
    Invalid(String),
}

impl Serialize for Word {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Word {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let value = String::deserialize(deserializer)?;
        Word::new(value).map_err(D::Error::custom)
    }
}

impl JsonSchema for Word {
    fn schema_name() -> String {
        "Word".to_owned()
    }

    fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> Schema {
        Schema::Object(SchemaObject {
            instance_type: Some(InstanceType::String.into()),
            string: Some(Box::new(StringValidation { pattern: Some(WORD_PATTERN.to_owned()), ..Default::default() })),
            ..Default::default()
        })
    }
}

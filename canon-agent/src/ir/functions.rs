use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::{
    ids::{DeltaId, FunctionId, ImplId, ModuleId, TraitFunctionId},
    types::{Receiver, ValuePort, Visibility},
    word::Word,
};

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Function {
    pub id: FunctionId,
    pub name: Word,
    pub module: ModuleId,
    pub impl_id: ImplId,
    pub trait_function: TraitFunctionId,
    pub visibility: Visibility,
    #[serde(default)]
    pub doc: Option<String>,
    #[serde(default)]
    pub lifetime_params: Vec<String>,
    #[serde(default)]
    pub receiver: Receiver,
    #[serde(default)]
    pub is_async: bool,
    #[serde(default)]
    pub is_unsafe: bool,
    #[serde(default)]
    pub generics: Vec<GenericParam>,
    #[serde(default)]
    pub where_clauses: Vec<WhereClause>,
    pub inputs: Vec<ValuePort>,
    pub outputs: Vec<ValuePort>,
    pub deltas: Vec<DeltaRef>,
    pub contract: FunctionContract,
    #[serde(default)]
    pub metadata: FunctionMetadata,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct DeltaRef {
    pub delta: DeltaId,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct FunctionContract {
    pub total: bool,
    pub deterministic: bool,
    pub explicit_inputs: bool,
    pub explicit_outputs: bool,
    pub effects_are_deltas: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct FunctionMetadata {
    #[serde(default)]
    pub bytecode_b64: Option<String>,
    #[serde(default)]
    pub ast: Option<JsonValue>,
    #[serde(default)]
    pub postconditions: Vec<Postcondition>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Postcondition {
    NonNegative { output: Word },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct FunctionSignature {
    pub name: Word,
    #[serde(default)]
    pub receiver: Receiver,
    #[serde(default)]
    pub is_async: bool,
    #[serde(default)]
    pub is_unsafe: bool,
    #[serde(default)]
    pub lifetime_params: Vec<String>,
    #[serde(default)]
    pub generics: Vec<GenericParam>,
    #[serde(default)]
    pub where_clauses: Vec<WhereClause>,
    #[serde(default)]
    pub doc: Option<String>,
    pub inputs: Vec<ValuePort>,
    pub outputs: Vec<ValuePort>,
    pub visibility: Visibility,
    pub trait_function: TraitFunctionId,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct GenericParam {
    pub name: Word,
    #[serde(default)]
    pub bounds: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct WhereClause {
    pub ty: String,
    #[serde(default)]
    pub bounds: Vec<String>,
}

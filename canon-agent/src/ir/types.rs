use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::word::Word;

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Receiver {
    #[default]
    None,
    SelfVal,
    SelfRef,
    SelfMutRef,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
    PubCrate,
    PubSuper,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StructKind {
    #[default]
    Normal,
    Tuple,
    Unit,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TypeKind {
    Scalar,
    Struct,
    Trait,
    Delta,
    External,
    Enum,
    Generic,
    Tuple,
    Slice,
    FnPtr,
    Never,
    SelfType,
    ImplTrait,
    DynTrait,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScalarType {
    F32,
    F64,
    I32,
    U32,
    Bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    #[default]
    None,
    Ref,
    MutRef,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
pub struct TypeRef {
    pub name: Word,
    pub kind: TypeKind,
    #[serde(default)]
    pub params: Vec<TypeRef>,
    #[serde(default)]
    pub ref_kind: RefKind,
    #[serde(default)]
    pub lifetime: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ValuePort {
    pub name: Word,
    pub ty: TypeRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CodeDelta {
    ApplyPatch { patch: String },
    Bash { command: String },
}

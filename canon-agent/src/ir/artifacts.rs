use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::{
    functions::{DeltaRef, GenericParam},
    ids::{EnumId, FunctionId, ImplId, ModuleId, StructId, TraitFunctionId, TraitId},
    types::{StructKind, TypeRef, ValuePort, Visibility},
    word::Word,
};

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub id: ModuleId,
    pub name: Word,
    pub visibility: Visibility,
    pub description: String,
    #[serde(default)]
    pub pub_uses: Vec<PubUseItem>,
    #[serde(default)]
    pub constants: Vec<ConstItem>,
    #[serde(default)]
    pub type_aliases: Vec<TypeAlias>,
    #[serde(default)]
    pub statics: Vec<StaticItem>,
    #[serde(default)]
    pub attributes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ModuleEdge {
    #[serde(rename = "from")]
    pub source: ModuleId,
    #[serde(rename = "to")]
    pub target: ModuleId,
    pub rationale: String,
    #[serde(default)]
    pub imported_types: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Struct {
    pub id: StructId,
    pub name: Word,
    pub module: ModuleId,
    pub visibility: Visibility,
    #[serde(default)]
    pub derives: Vec<String>,
    #[serde(default)]
    pub doc: Option<String>,
    #[serde(default)]
    pub kind: StructKind,
    pub fields: Vec<Field>,
    pub history: Vec<DeltaRef>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: Word,
    pub ty: TypeRef,
    pub visibility: Visibility,
    #[serde(default)]
    pub doc: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Trait {
    pub id: TraitId,
    pub name: Word,
    pub module: ModuleId,
    pub visibility: Visibility,
    #[serde(default)]
    pub generic_params: Vec<GenericParam>,
    pub functions: Vec<TraitFunction>,
    #[serde(default)]
    pub supertraits: Vec<String>,
    #[serde(default)]
    pub associated_types: Vec<AssociatedType>,
    #[serde(default)]
    pub associated_consts: Vec<AssociatedConst>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct TraitFunction {
    pub id: TraitFunctionId,
    pub name: Word,
    pub inputs: Vec<ValuePort>,
    pub outputs: Vec<ValuePort>,
    #[serde(default)]
    pub default_body: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ImplBlock {
    pub id: ImplId,
    pub module: ModuleId,
    pub struct_id: StructId,
    pub trait_id: TraitId,
    pub functions: Vec<ImplFunctionBinding>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ImplFunctionBinding {
    pub trait_fn: TraitFunctionId,
    pub function: FunctionId,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct EnumNode {
    pub id: EnumId,
    pub name: Word,
    pub module: ModuleId,
    pub visibility: Visibility,
    pub variants: Vec<EnumVariant>,
    pub history: Vec<DeltaRef>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct EnumVariant {
    pub name: Word,
    pub fields: EnumVariantFields,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct AssociatedType {
    pub name: Word,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct AssociatedConst {
    pub name: Word,
    pub ty: TypeRef,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnumVariantFields {
    Unit,
    Tuple { types: Vec<TypeRef> },
    Struct { fields: Vec<Field> },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct PubUseItem {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConstItem {
    pub name: Word,
    pub ty: TypeRef,
    pub value_expr: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct StaticItem {
    pub name: String,
    pub ty: TypeRef,
    pub value_expr: String,
    #[serde(default)]
    pub mutable: bool,
    #[serde(default)]
    pub doc: Option<String>,
    pub visibility: Visibility,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct TypeAlias {
    pub name: Word,
    pub target: TypeRef,
}

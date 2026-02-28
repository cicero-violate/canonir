use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    ids::{FunctionId, GpuFunctionId},
    types::ScalarType,
    word::Word,
};

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct GpuFunction {
    pub id: GpuFunctionId,
    pub function: FunctionId,
    pub inputs: Vec<VectorPort>,
    pub outputs: Vec<VectorPort>,
    pub properties: GpuProperties,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct VectorPort {
    pub name: Word,
    pub scalar: ScalarType,
    pub lanes: u32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct GpuProperties {
    pub pure: bool,
    pub no_io: bool,
    pub no_alloc: bool,
    pub no_branch: bool,
}

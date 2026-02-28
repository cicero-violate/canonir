use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use super::{
    ids::{CallEdgeId, FunctionId, SystemGraphId, SystemNodeId, TickGraphId},
    word::Word,
};
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct CallEdge {
    pub id: CallEdgeId,
    pub caller: FunctionId,
    pub callee: FunctionId,
    pub rationale: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ExecutionGraph {
    pub id: TickGraphId,
    pub name: Word,
    pub nodes: Vec<FunctionId>,
    pub edges: Vec<TickEdge>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct TickEdge {
    pub from: FunctionId,
    pub to: FunctionId,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct SystemGraph {
    pub id: SystemGraphId,
    pub name: Word,
    pub nodes: Vec<SystemNode>,
    pub edges: Vec<SystemEdge>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct SystemNode {
    pub id: SystemNodeId,
    pub function: FunctionId,
    pub kind: SystemNodeKind,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub enum SystemNodeKind {
    Function,
    Gate,
    Persist,
    Materialize,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct SystemEdge {
    pub from: SystemNodeId,
    pub to: SystemNodeId,
    pub kind: SystemEdgeKind,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub enum SystemEdgeKind {
    Control,
    Data,
}

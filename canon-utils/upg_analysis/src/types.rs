use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
    Field,
    Param,
    Variable,
    Module,
    Type,
    BasicBlock,
    CallSite,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EdgeKind {
    Contains,
    HasField,
    HasMethod,
    HasBlock,
    HasParam,
    Imports,
    Export,
    PublicUse,
    Flow,
    Call,
    Return,
    Unwind,
    Implements,
    ForType,
    UsesType,
    Bounds,
    Assign,
    Propagates,
    ArgToParam,
    Returns,
    ErrorToFunction,
    ErrorToBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: u32,
    pub kind: NodeKind,
    pub symbol: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SpanRange {
    pub lo: u32,
    pub hi: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub src: u32,
    pub dst: u32,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub project: String,
    pub node_count: u32,
    pub edge_count: u32,
    pub def_count: u32,
    pub generated_by: String,
}

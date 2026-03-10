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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
    pub file_id: Option<u32>,
    pub parent: Option<u32>,
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
    pub schema_version: u32,
    pub generated_by: String,
}

pub const SCHEMA_VERSION: u32 = 1;

pub fn parse_node_kind(raw: &str) -> Result<NodeKind, String> {
    match raw {
        "FUNCTION" => Ok(NodeKind::Function),
        "METHOD" => Ok(NodeKind::Method),
        "STRUCT" => Ok(NodeKind::Struct),
        "ENUM" => Ok(NodeKind::Enum),
        "TRAIT" => Ok(NodeKind::Trait),
        "IMPL" => Ok(NodeKind::Impl),
        "FIELD" => Ok(NodeKind::Field),
        "PARAM" => Ok(NodeKind::Param),
        "VARIABLE" => Ok(NodeKind::Variable),
        "MODULE" => Ok(NodeKind::Module),
        "TYPE" => Ok(NodeKind::Type),
        "BASIC_BLOCK" => Ok(NodeKind::BasicBlock),
        "CALL_SITE" => Ok(NodeKind::CallSite),
        "ERROR" => Ok(NodeKind::Error),
        _ => Err(format!("unknown node kind: {raw}")),
    }
}

pub fn parse_edge_kind(raw: &str) -> Result<EdgeKind, String> {
    match raw {
        "HAS_FIELD" => Ok(EdgeKind::HasField),
        "HAS_METHOD" => Ok(EdgeKind::HasMethod),
        "HAS_BLOCK" => Ok(EdgeKind::HasBlock),
        "HAS_PARAM" => Ok(EdgeKind::HasParam),
        "IMPORTS" => Ok(EdgeKind::Imports),
        "FLOW" => Ok(EdgeKind::Flow),
        "CALL" => Ok(EdgeKind::Call),
        "RETURN" => Ok(EdgeKind::Return),
        "UNWIND" => Ok(EdgeKind::Unwind),
        "IMPLEMENTS" => Ok(EdgeKind::Implements),
        "USES_TYPE" => Ok(EdgeKind::UsesType),
        "BOUNDS" => Ok(EdgeKind::Bounds),
        "ASSIGN" => Ok(EdgeKind::Assign),
        "PROPAGATES" => Ok(EdgeKind::Propagates),
        "ARG_TO_PARAM" => Ok(EdgeKind::ArgToParam),
        "RETURNS" => Ok(EdgeKind::Returns),
        "ERROR_TO_FUNCTION" => Ok(EdgeKind::ErrorToFunction),
        "ERROR_TO_BLOCK" => Ok(EdgeKind::ErrorToBlock),
        "CONTAINS" => Ok(EdgeKind::Contains),
        "EXPORT" => Ok(EdgeKind::Export),
        "FOR_TYPE" => Ok(EdgeKind::ForType),
        "PUBLIC_USE" => Ok(EdgeKind::PublicUse),
        _ => Err(format!("unknown edge kind: {raw}")),
    }
}

pub fn node_kind_str(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Function => "FUNCTION",
        NodeKind::Method => "METHOD",
        NodeKind::Struct => "STRUCT",
        NodeKind::Enum => "ENUM",
        NodeKind::Trait => "TRAIT",
        NodeKind::Impl => "IMPL",
        NodeKind::Field => "FIELD",
        NodeKind::Param => "PARAM",
        NodeKind::Variable => "VARIABLE",
        NodeKind::Module => "MODULE",
        NodeKind::Type => "TYPE",
        NodeKind::BasicBlock => "BASIC_BLOCK",
        NodeKind::CallSite => "CALL_SITE",
        NodeKind::Error => "ERROR",
    }
}

pub fn edge_kind_str(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::HasField => "HAS_FIELD",
        EdgeKind::HasMethod => "HAS_METHOD",
        EdgeKind::HasBlock => "HAS_BLOCK",
        EdgeKind::HasParam => "HAS_PARAM",
        EdgeKind::Imports => "IMPORTS",
        EdgeKind::Flow => "FLOW",
        EdgeKind::Call => "CALL",
        EdgeKind::Return => "RETURN",
        EdgeKind::Unwind => "UNWIND",
        EdgeKind::Implements => "IMPLEMENTS",
        EdgeKind::UsesType => "USES_TYPE",
        EdgeKind::Bounds => "BOUNDS",
        EdgeKind::Assign => "ASSIGN",
        EdgeKind::Propagates => "PROPAGATES",
        EdgeKind::ArgToParam => "ARG_TO_PARAM",
        EdgeKind::Returns => "RETURNS",
        EdgeKind::ErrorToFunction => "ERROR_TO_FUNCTION",
        EdgeKind::ErrorToBlock => "ERROR_TO_BLOCK",
        EdgeKind::Contains => "CONTAINS",
        EdgeKind::Export => "EXPORT",
        EdgeKind::ForType => "FOR_TYPE",
        EdgeKind::PublicUse => "PUBLIC_USE",
    }
}

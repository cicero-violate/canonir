use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    HasField,
    HasMethod,
    HasBlock,
    HasParam,
    Imports,
    Flow,
    Call,
    Return,
    Unwind,
    Implements,
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
    pub generated_by: String,
}

#[derive(Debug, Clone)]
pub struct AnalysisGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub row_ptr: Vec<u32>,
    pub col_idx: Vec<u32>,
    pub repair_surface: Value,
    pub errors: Value,
    pub metadata: Metadata,
    pub node_kinds: Vec<NodeKind>,
    pub edge_kinds: Vec<EdgeKind>,
}

pub fn load_dir(dir: &Path) -> Result<AnalysisGraph> {
    let nodes = read_nodes_csv(dir.join("nodes.csv"))?;
    let edges = read_edges_csv(dir.join("edges.csv"))?;
    let row_ptr = read_bin_u32(dir.join("csr_row_ptr.bin"))?;
    let col_idx = read_bin_u32(dir.join("csr_col_idx.bin"))?;
    let repair_surface = read_json(dir.join("repair_surface.json"));
    let errors = read_json(dir.join("errors.json"));
    let metadata = read_metadata(dir.join("metadata.json"))?;
    let node_kinds = read_node_kinds(dir.join("node_kinds.txt"))?;
    let edge_kinds = read_edge_kinds(dir.join("edge_kinds.txt"))?;

    Ok(AnalysisGraph {
        nodes,
        edges,
        row_ptr,
        col_idx,
        repair_surface,
        errors,
        metadata,
        node_kinds,
        edge_kinds,
    })
}

fn read_nodes_csv(path: PathBuf) -> Result<Vec<Node>> {
    let content = fs::read_to_string(path)?;
    let mut nodes = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 6 {
            return Err(anyhow!("invalid nodes.csv line"));
        }
        let id = parts[0].parse::<u32>()?;
        let kind = parse_node_kind(parts[1])?;
        let line_no = parts[parts.len() - 2].parse::<u32>()?;
        let col = parts[parts.len() - 1].parse::<u32>()?;
        let file = parts[parts.len() - 3].to_string();
        let symbol = parts[2..parts.len() - 3].join(",");
        nodes.push(Node {
            id,
            kind,
            symbol,
            file,
            line: line_no,
            column: col,
        });
    }
    Ok(nodes)
}

fn read_edges_csv(path: PathBuf) -> Result<Vec<Edge>> {
    let content = fs::read_to_string(path)?;
    let mut edges = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 3 {
            return Err(anyhow!("invalid edges.csv line"));
        }
        let src = parts[0].parse::<u32>()?;
        let dst = parts[1].parse::<u32>()?;
        let kind = parse_edge_kind(parts[2])?;
        edges.push(Edge { src, dst, kind });
    }
    Ok(edges)
}

fn read_bin_u32(path: PathBuf) -> Result<Vec<u32>> {
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.len() % 4 != 0 {
        return Err(anyhow!("invalid u32 binary length"));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

fn read_json(path: PathBuf) -> Value {
    fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
}

fn read_metadata(path: PathBuf) -> Result<Metadata> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn read_node_kinds(path: PathBuf) -> Result<Vec<NodeKind>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in content.lines() {
        let kind = parse_node_kind(line)?;
        out.push(kind);
    }
    Ok(out)
}

fn read_edge_kinds(path: PathBuf) -> Result<Vec<EdgeKind>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in content.lines() {
        let kind = parse_edge_kind(line)?;
        out.push(kind);
    }
    Ok(out)
}

fn parse_node_kind(raw: &str) -> Result<NodeKind> {
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
        _ => Err(anyhow!("unknown node kind")),
    }
}

fn parse_edge_kind(raw: &str) -> Result<EdgeKind> {
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
        _ => Err(anyhow!("unknown edge kind")),
    }
}

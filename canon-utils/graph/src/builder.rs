use canon_types::{Edge, EdgeKind, Node, NodeKind, SpanRange};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub struct KernelGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub spans: HashMap<String, SpanRange>,
    pub files: Vec<String>,
    pub symbol_to_id: HashMap<String, u32>,
    pub id_to_kind: HashMap<u32, NodeKind>,
}

pub fn build_from_tlog(tlog_path: &Path) -> Result<KernelGraph> {
    let file = File::open(tlog_path)?;
    let reader = BufReader::new(file);

    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut spans: HashMap<String, SpanRange> = HashMap::new();
    let mut files: Vec<String> = Vec::new();
    let mut symbol_to_id: HashMap<String, u32> = HashMap::new();
    let mut id_to_kind: HashMap<u32, NodeKind> = HashMap::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)?;
        let Some(tag) = value.get("t").and_then(|v| v.as_str()) else {
            continue;
        };
        match tag {
            "SESSION" => {
                nodes.clear();
                edges.clear();
                spans.clear();
                files.clear();
                symbol_to_id.clear();
                id_to_kind.clear();
            }
            "N" => {
                let sym = value.get("sym").and_then(|v| v.as_str()).unwrap_or("");
                let kind_raw = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                let file = value.get("file").and_then(|v| v.as_str()).unwrap_or("");
                let line_no = value.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let col_no = value.get("col").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let lo = value.get("lo").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let hi = value.get("hi").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                if sym.is_empty() || file.is_empty() {
                    continue;
                }
                let kind = parse_node_kind(kind_raw)?;
                let id = nodes.len() as u32;
                nodes.push(Node {
                    id,
                    kind,
                    symbol: sym.to_string(),
                    file: file.to_string(),
                    line: line_no,
                    column: col_no,
                    file_id: None,
                    parent: None,
                });
                spans.insert(sym.to_string(), SpanRange { lo, hi });
                symbol_to_id.insert(sym.to_string(), id);
                id_to_kind.insert(id, kind);
            }
            "E" => {
                let src_sym = value.get("src").and_then(|v| v.as_str()).unwrap_or("");
                let dst_sym = value.get("dst").and_then(|v| v.as_str()).unwrap_or("");
                let kind_raw = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                let Some(&src) = symbol_to_id.get(src_sym) else {
                    continue;
                };
                let Some(&dst) = symbol_to_id.get(dst_sym) else {
                    continue;
                };
                let kind = parse_edge_kind(kind_raw)?;
                edges.push(Edge { src, dst, kind });
            }
            "F" => {
                let path = value.get("path").and_then(|v| v.as_str()).unwrap_or("");
                if !path.is_empty() {
                    files.push(path.to_string());
                }
            }
            _ => {}
        }
    }

    Ok(KernelGraph {
        nodes,
        edges,
        spans,
        files,
        symbol_to_id,
        id_to_kind,
    })
}

fn parse_node_kind(raw: &str) -> Result<NodeKind> {
    let kind = match raw {
        "FUNCTION" => NodeKind::Function,
        "METHOD" => NodeKind::Method,
        "STRUCT" => NodeKind::Struct,
        "ENUM" => NodeKind::Enum,
        "TRAIT" => NodeKind::Trait,
        "IMPL" => NodeKind::Impl,
        "FIELD" => NodeKind::Field,
        "PARAM" => NodeKind::Param,
        "VARIABLE" => NodeKind::Variable,
        "MODULE" => NodeKind::Module,
        "TYPE" => NodeKind::Type,
        "BASIC_BLOCK" => NodeKind::BasicBlock,
        "CALL_SITE" => NodeKind::CallSite,
        "ERROR" => NodeKind::Error,
        other => return Err(anyhow!("unknown node kind {other}")),
    };
    Ok(kind)
}

fn parse_edge_kind(raw: &str) -> Result<EdgeKind> {
    let kind = match raw {
        "CONTAINS" => EdgeKind::Contains,
        "HAS_FIELD" => EdgeKind::HasField,
        "HAS_METHOD" => EdgeKind::HasMethod,
        "HAS_BLOCK" => EdgeKind::HasBlock,
        "HAS_PARAM" => EdgeKind::HasParam,
        "IMPORTS" => EdgeKind::Imports,
        "EXPORT" => EdgeKind::Export,
        "PUBLIC_USE" => EdgeKind::PublicUse,
        "FLOW" => EdgeKind::Flow,
        "CALL" => EdgeKind::Call,
        "RETURN" => EdgeKind::Return,
        "UNWIND" => EdgeKind::Unwind,
        "IMPLEMENTS" => EdgeKind::Implements,
        "FOR_TYPE" => EdgeKind::ForType,
        "USES_TYPE" => EdgeKind::UsesType,
        "BOUNDS" => EdgeKind::Bounds,
        "ASSIGN" => EdgeKind::Assign,
        "PROPAGATES" => EdgeKind::Propagates,
        "ARG_TO_PARAM" => EdgeKind::ArgToParam,
        "RETURNS" => EdgeKind::Returns,
        "ERROR_TO_FUNCTION" => EdgeKind::ErrorToFunction,
        "ERROR_TO_BLOCK" => EdgeKind::ErrorToBlock,
        other => return Err(anyhow!("unknown edge kind {other}")),
    };
    Ok(kind)
}

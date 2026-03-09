use crate::types::{Edge, Node};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub output_dir: PathBuf,
}

pub fn write_outputs(graph: &crate::extract::UpgGraph, output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir)?;
    write_nodes_csv(output_dir, &graph.nodes)?;
    write_edges_csv(output_dir, &graph.edges)?;
    write_kinds(output_dir)?;
    write_bin_u32(output_dir.join("csr_row_ptr.bin"), &graph.csr.row_ptr)?;
    write_bin_u32(output_dir.join("csr_col_idx.bin"), &graph.csr.col_idx)?;
    let metadata_path = output_dir.join("metadata.json");
    let file = fs::File::create(metadata_path)?;
    serde_json::to_writer_pretty(file, &graph.metadata)
        .map_err(|err| anyhow!("failed to write metadata.json: {err}"))
}

fn write_nodes_csv(output_dir: &Path, nodes: &[Node]) -> Result<()> {
    let path = output_dir.join("nodes.csv");
    let mut file = fs::File::create(path)?;
    writeln!(file, "node_id,node_kind,symbol,file,line,column")?;
    for node in nodes {
        let symbol = sanitize_csv_field(&node.symbol);
        let file_field = sanitize_csv_field(&node.file);
        writeln!(
            file,
            "{},{},{},{},{},{}",
            node.id,
            node_kind_str(node.kind),
            symbol,
            file_field,
            node.line,
            node.column
        )?;
    }
    Ok(())
}

fn write_edges_csv(output_dir: &Path, edges: &[Edge]) -> Result<()> {
    let path = output_dir.join("edges.csv");
    let mut file = fs::File::create(path)?;
    writeln!(file, "src_id,dst_id,edge_kind")?;
    for edge in edges {
        writeln!(
            file,
            "{},{},{}",
            edge.src,
            edge.dst,
            edge_kind_str(edge.kind)
        )?;
    }
    Ok(())
}

fn node_kind_str(kind: crate::types::NodeKind) -> &'static str {
    match kind {
        crate::types::NodeKind::Function => "FUNCTION",
        crate::types::NodeKind::Method => "METHOD",
        crate::types::NodeKind::Struct => "STRUCT",
        crate::types::NodeKind::Enum => "ENUM",
        crate::types::NodeKind::Trait => "TRAIT",
        crate::types::NodeKind::Impl => "IMPL",
        crate::types::NodeKind::Field => "FIELD",
        crate::types::NodeKind::Param => "PARAM",
        crate::types::NodeKind::Variable => "VARIABLE",
        crate::types::NodeKind::Module => "MODULE",
        crate::types::NodeKind::Type => "TYPE",
        crate::types::NodeKind::BasicBlock => "BASIC_BLOCK",
        crate::types::NodeKind::CallSite => "CALL_SITE",
        crate::types::NodeKind::Error => "ERROR",
    }
}

fn edge_kind_str(kind: crate::types::EdgeKind) -> &'static str {
    match kind {
        crate::types::EdgeKind::HasField => "HAS_FIELD",
        crate::types::EdgeKind::HasMethod => "HAS_METHOD",
        crate::types::EdgeKind::HasBlock => "HAS_BLOCK",
        crate::types::EdgeKind::HasParam => "HAS_PARAM",
        crate::types::EdgeKind::Imports => "IMPORTS",
        crate::types::EdgeKind::Flow => "FLOW",
        crate::types::EdgeKind::Call => "CALL",
        crate::types::EdgeKind::Return => "RETURN",
        crate::types::EdgeKind::Unwind => "UNWIND",
        crate::types::EdgeKind::Implements => "IMPLEMENTS",
        crate::types::EdgeKind::UsesType => "USES_TYPE",
        crate::types::EdgeKind::Bounds => "BOUNDS",
        crate::types::EdgeKind::Assign => "ASSIGN",
        crate::types::EdgeKind::Propagates => "PROPAGATES",
        crate::types::EdgeKind::ArgToParam => "ARG_TO_PARAM",
        crate::types::EdgeKind::Returns => "RETURNS",
        crate::types::EdgeKind::ErrorToFunction => "ERROR_TO_FUNCTION",
        crate::types::EdgeKind::ErrorToBlock => "ERROR_TO_BLOCK",
    }
}

fn sanitize_csv_field(raw: &str) -> String {
    let mut out = raw
        .replace('\n', " ")
        .replace('\r', " ")
        .replace('\0', "");
    if out.contains(',') {
        out = out.replace(',', ";");
    }
    out
}

fn write_kinds(output_dir: &Path) -> Result<()> {
    let node_kinds = [
        "FUNCTION",
        "METHOD",
        "STRUCT",
        "ENUM",
        "TRAIT",
        "IMPL",
        "FIELD",
        "PARAM",
        "VARIABLE",
        "MODULE",
        "TYPE",
        "BASIC_BLOCK",
        "CALL_SITE",
        "ERROR",
    ];
    let edge_kinds = [
        "HAS_FIELD",
        "HAS_METHOD",
        "HAS_BLOCK",
        "HAS_PARAM",
        "IMPORTS",
        "FLOW",
        "CALL",
        "RETURN",
        "UNWIND",
        "IMPLEMENTS",
        "USES_TYPE",
        "BOUNDS",
        "ASSIGN",
        "PROPAGATES",
        "ARG_TO_PARAM",
        "RETURNS",
        "ERROR_TO_FUNCTION",
        "ERROR_TO_BLOCK",
    ];
    fs::write(output_dir.join("node_kinds.txt"), node_kinds.join("\n"))?;
    fs::write(output_dir.join("edge_kinds.txt"), edge_kinds.join("\n"))?;
    Ok(())
}

fn write_bin_u32(path: PathBuf, values: &[u32]) -> Result<()> {
    let mut file = fs::File::create(path)?;
    for &value in values {
        file.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}

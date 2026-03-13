use anyhow::{anyhow, Result};
use csv::Writer;
use memmap2::Mmap;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::graph::graph_types::{EdgeRow, ModuleNode, NodeRow};
use crate::graph::graph_builder::module_prefixes;
use crate::artifacts::cache::GraphCache;

pub fn is_graph_bin_fresh(graph_bin: &Path, tlog_path: &Path) -> bool {
    let tlog_idx = tlog_path.with_extension("tlog.idx");
    let bin_meta = graph_bin.metadata().and_then(|m| m.modified());
    let idx_meta = tlog_idx.metadata().and_then(|m| m.modified());
    match (bin_meta, idx_meta) {
        (Ok(bin), Ok(idx)) => bin >= idx,
        _ => false,
    }
}

pub fn emit_graph_bin(path: &Path, nodes: &[NodeRow], edges: &[EdgeRow], files: &[String]) -> Result<()> {
    const MAGIC: &[u8; 4] = b"CGBN";
    const VERSION: u32 = 1;
    const HEADER_SIZE: usize = 32;
    const NODE_RECORD_SIZE: usize = 21;
    const EDGE_RECORD_SIZE: usize = 9;
    const NO_FILE_ID: u32 = u32::MAX;
    const NO_LINE: u32 = u32::MAX;

    let n_nodes = nodes.len() as u32;
    let n_edges = edges.len() as u32;
    let n_files = files.len() as u32;

    let mut file_index: HashMap<&str, u32> = HashMap::new();
    for (idx, path) in files.iter().enumerate() {
        file_index.insert(path.as_str(), idx as u32);
    }

    let mut string_table: Vec<u8> = Vec::new();
    let mut string_offsets: HashMap<&str, (u32, u32)> = HashMap::new();

    for path in files {
        let offset = string_table.len() as u32;
        let bytes = path.as_bytes();
        string_table.extend_from_slice(bytes);
        string_table.push(0);
        string_offsets.insert(path.as_str(), (offset, bytes.len() as u32));
    }

    for node in nodes {
        if string_offsets.contains_key(node.symbol.as_str()) {
            continue;
        }
        let offset = string_table.len() as u32;
        let bytes = node.symbol.as_bytes();
        string_table.extend_from_slice(bytes);
        string_table.push(0);
        string_offsets.insert(node.symbol.as_str(), (offset, bytes.len() as u32));
    }

    let str_table_offset = HEADER_SIZE as u32
        + n_nodes
            .checked_mul(NODE_RECORD_SIZE as u32)
            .ok_or_else(|| anyhow!("graph.bin node section too large"))?
        + n_edges
            .checked_mul(EDGE_RECORD_SIZE as u32)
            .ok_or_else(|| anyhow!("graph.bin edge section too large"))?;

    let mut out = Vec::with_capacity(
        HEADER_SIZE + (n_nodes as usize * NODE_RECORD_SIZE) + (n_edges as usize * EDGE_RECORD_SIZE) + string_table.len(),
    );

    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&n_nodes.to_le_bytes());
    out.extend_from_slice(&n_edges.to_le_bytes());
    out.extend_from_slice(&n_files.to_le_bytes());
    out.extend_from_slice(&str_table_offset.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);

    for node in nodes {
        let (sym_off, sym_len) = string_offsets
            .get(node.symbol.as_str())
            .copied()
            .unwrap_or((0, 0));
        let file_id = node
            .file_id
            .and_then(|id| files.get(id as usize))
            .and_then(|p| file_index.get(p.as_str()))
            .copied()
            .unwrap_or(NO_FILE_ID);
        out.extend_from_slice(&node.id.to_le_bytes());
        out.push(node_kind_code(node.kind.as_str()));
        out.extend_from_slice(&file_id.to_le_bytes());
        out.extend_from_slice(&node.line.unwrap_or(NO_LINE).to_le_bytes());
        out.extend_from_slice(&sym_off.to_le_bytes());
        out.extend_from_slice(&sym_len.to_le_bytes());
    }

    for edge in edges {
        out.extend_from_slice(&edge.src.to_le_bytes());
        out.extend_from_slice(&edge.dst.to_le_bytes());
        out.push(edge_kind_code(edge.kind.as_str()));
    }

    out.extend_from_slice(&string_table);

    fs::write(path, out)?;
    Ok(())
}

fn node_kind_code(kind: &str) -> u8 {
    match kind {
        "FUNCTION" => 1,
        "METHOD" => 2,
        "STRUCT" => 3,
        "ENUM" => 4,
        "TRAIT" => 5,
        "IMPL" => 6,
        "FIELD" => 7,
        "PARAM" => 8,
        "VARIABLE" => 9,
        "MODULE" => 10,
        "TYPE" => 11,
        "BASIC_BLOCK" => 12,
        "CALL_SITE" => 13,
        "ERROR" => 14,
        _ => 0,
    }
}

fn edge_kind_code(kind: &str) -> u8 {
    match kind {
        "CONTAINS" => 1,
        "HAS_FIELD" => 2,
        "HAS_METHOD" => 3,
        "HAS_BLOCK" => 4,
        "HAS_PARAM" => 5,
        "IMPORTS" => 6,
        "EXPORT" => 7,
        "PUBLIC_USE" => 8,
        "FLOW" => 9,
        "CALL" => 10,
        "RETURN" => 11,
        "UNWIND" => 12,
        "IMPLEMENTS" => 13,
        "FOR_TYPE" => 14,
        "USES_TYPE" => 15,
        "BOUNDS" => 16,
        "ASSIGN" => 17,
        "PROPAGATES" => 18,
        "ARG_TO_PARAM" => 19,
        "RETURNS" => 20,
        "ERROR_TO_FUNCTION" => 21,
        "ERROR_TO_BLOCK" => 22,
        _ => 0,
    }
}

pub fn load_graph_bin(path: &Path) -> Result<(Vec<NodeRow>, Vec<EdgeRow>, Vec<String>)> {
    const HEADER_SIZE: usize = 32;
    const NODE_RECORD_SIZE: usize = 21;
    const EDGE_RECORD_SIZE: usize = 9;
    const NO_FILE_ID: u32 = u32::MAX;
    const NO_LINE: u32 = u32::MAX;

    let file = fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];
    if data.len() < HEADER_SIZE {
        return Err(anyhow!("graph.bin too small"));
    }
    if &data[0..4] != b"CGBN" {
        return Err(anyhow!("graph.bin magic mismatch"));
    }
    let version = u32::from_le_bytes(data[4..8].try_into()?);
    if version != 1 {
        return Err(anyhow!("graph.bin version mismatch"));
    }
    let n_nodes = u32::from_le_bytes(data[8..12].try_into()?) as usize;
    let n_edges = u32::from_le_bytes(data[12..16].try_into()?) as usize;
    let n_files = u32::from_le_bytes(data[16..20].try_into()?) as usize;
    let str_table_offset = u32::from_le_bytes(data[20..24].try_into()?) as usize;

    let nodes_offset = HEADER_SIZE;
    let edges_offset = nodes_offset + n_nodes * NODE_RECORD_SIZE;
    let expected_str_offset = edges_offset + n_edges * EDGE_RECORD_SIZE;
    if str_table_offset != expected_str_offset {
        return Err(anyhow!("graph.bin string table offset mismatch"));
    }
    if data.len() < str_table_offset {
        return Err(anyhow!("graph.bin truncated"));
    }

    let string_table = &data[str_table_offset..];

    let mut files: Vec<String> = Vec::with_capacity(n_files);
    let mut cursor = 0usize;
    for _ in 0..n_files {
        let start = cursor;
        while cursor < string_table.len() && string_table[cursor] != 0 {
            cursor += 1;
        }
        let s = std::str::from_utf8(&string_table[start..cursor]).unwrap_or("").to_string();
        files.push(s);
        cursor = cursor.saturating_add(1);
    }

    let mut nodes: Vec<NodeRow> = Vec::with_capacity(n_nodes);
    let mut pos = nodes_offset;
    for _ in 0..n_nodes {
        let id = u32::from_le_bytes(data[pos..pos + 4].try_into()?);
        let kind_code = data[pos + 4];
        let file_id = u32::from_le_bytes(data[pos + 5..pos + 9].try_into()?);
        let line = u32::from_le_bytes(data[pos + 9..pos + 13].try_into()?);
        let sym_off = u32::from_le_bytes(data[pos + 13..pos + 17].try_into()?) as usize;
        let sym_len = u32::from_le_bytes(data[pos + 17..pos + 21].try_into()?) as usize;
        pos += NODE_RECORD_SIZE;

        let symbol = if sym_len == 0 {
            String::new()
        } else {
            let end = sym_off.saturating_add(sym_len);
            if end <= string_table.len() {
                std::str::from_utf8(&string_table[sym_off..end])
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            }
        };

        nodes.push(NodeRow {
            id,
            kind: node_kind_str(kind_code).to_string(),
            symbol,
            file_id: if file_id == NO_FILE_ID { None } else { Some(file_id) },
            line: if line == NO_LINE { None } else { Some(line) },
        });
    }

    let mut edges: Vec<EdgeRow> = Vec::with_capacity(n_edges);
    let mut pos = edges_offset;
    for _ in 0..n_edges {
        let src = u32::from_le_bytes(data[pos..pos + 4].try_into()?);
        let dst = u32::from_le_bytes(data[pos + 4..pos + 8].try_into()?);
        let kind_code = data[pos + 8];
        pos += EDGE_RECORD_SIZE;
        edges.push(EdgeRow {
            src,
            dst,
            kind: edge_kind_str(kind_code).to_string(),
        });
    }

    Ok((nodes, edges, files))
}

fn node_kind_str(code: u8) -> &'static str {
    match code {
        1 => "FUNCTION",
        2 => "METHOD",
        3 => "STRUCT",
        4 => "ENUM",
        5 => "TRAIT",
        6 => "IMPL",
        7 => "FIELD",
        8 => "PARAM",
        9 => "VARIABLE",
        10 => "MODULE",
        11 => "TYPE",
        12 => "BASIC_BLOCK",
        13 => "CALL_SITE",
        14 => "ERROR",
        _ => "UNKNOWN",
    }
}

fn edge_kind_str(code: u8) -> &'static str {
    match code {
        1 => "CONTAINS",
        2 => "HAS_FIELD",
        3 => "HAS_METHOD",
        4 => "HAS_BLOCK",
        5 => "HAS_PARAM",
        6 => "IMPORTS",
        7 => "EXPORT",
        8 => "PUBLIC_USE",
        9 => "FLOW",
        10 => "CALL",
        11 => "RETURN",
        12 => "UNWIND",
        13 => "IMPLEMENTS",
        14 => "FOR_TYPE",
        15 => "USES_TYPE",
        16 => "BOUNDS",
        17 => "ASSIGN",
        18 => "PROPAGATES",
        19 => "ARG_TO_PARAM",
        20 => "RETURNS",
        21 => "ERROR_TO_FUNCTION",
        22 => "ERROR_TO_BLOCK",
        _ => "UNKNOWN",
    }
}

pub fn emit_cfg_csv(out_dir: &Path, cfg: &[EdgeRow]) -> Result<()> {
    let path = out_dir.join("cfg.csv");
    let mut buf = String::with_capacity(cfg.len() * 24 + 32);
    buf.push_str("src_block,dst_block,edge_kind\n");
    for edge in cfg {
        buf.push_str(&edge.src.to_string());
        buf.push(',');
        buf.push_str(&edge.dst.to_string());
        buf.push(',');
        buf.push_str(&edge.kind);
        buf.push('\n');
    }
    fs::write(path, buf)?;
    Ok(())
}

pub fn emit_cfg_full_csv(
    out_dir: &Path,
    cfg: &[EdgeRow],
    nodes: &[NodeRow],
    files: &[String],
) -> Result<()> {
    let path = out_dir.join("cfg_full.csv");
    let mut buf = String::with_capacity(cfg.len() * 64 + 64);
    buf.push_str("src_block,dst_block,edge_kind,src_symbol,dst_symbol,src_file,dst_file,src_line,dst_line\n");
    for edge in cfg {
        let src_node = nodes.iter().find(|n| n.id == edge.src);
        let dst_node = nodes.iter().find(|n| n.id == edge.dst);
        let src_sym = src_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let dst_sym = dst_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let src_file = src_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let dst_file = dst_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let src_line = src_node.and_then(|n| n.line).unwrap_or(0);
        let dst_line = dst_node.and_then(|n| n.line).unwrap_or(0);
        buf.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            edge.src,
            edge.dst,
            edge.kind,
            sanitize_csv_field(src_sym),
            sanitize_csv_field(dst_sym),
            sanitize_csv_field(src_file),
            sanitize_csv_field(dst_file),
            src_line,
            dst_line
        ));
    }
    fs::write(path, buf)?;
    Ok(())
}

pub fn emit_callgraph_csv(
    out_dir: &Path,
    callgraph: &[(u32, u32)],
    nodes: &[NodeRow],
    files: &[String],
) -> Result<()> {
    let path = out_dir.join("callgraph.csv");
    let mut buf = String::with_capacity(callgraph.len() * 64 + 64);
    buf.push_str("caller_node,callee_node,caller_symbol,callee_symbol,caller_file,callee_file\n");
    for (caller, callee) in callgraph {
        let caller_node = nodes.iter().find(|n| n.id == *caller);
        let callee_node = nodes.iter().find(|n| n.id == *callee);
        let caller_sym = caller_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let callee_sym = callee_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let caller_file = caller_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let callee_file = callee_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        buf.push_str(&format!("{caller},{callee},{caller_sym},{callee_sym},{caller_file},{callee_file}\n"));
    }
    fs::write(path, buf)?;
    Ok(())
}

pub fn emit_callgraph_full_csv(
    out_dir: &Path,
    callgraph: &[(u32, u32)],
    nodes: &[NodeRow],
    files: &[String],
) -> Result<()> {
    let path = out_dir.join("callgraph_full.csv");
    let mut buf = String::with_capacity(callgraph.len() * 80 + 64);
    buf.push_str("caller_node,callee_node,caller_symbol,callee_symbol,caller_file,callee_file,caller_line,callee_line\n");
    for (caller, callee) in callgraph {
        let caller_node = nodes.iter().find(|n| n.id == *caller);
        let callee_node = nodes.iter().find(|n| n.id == *callee);
        let caller_sym = caller_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let callee_sym = callee_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let caller_file = caller_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let callee_file = callee_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let caller_line = caller_node.and_then(|n| n.line).unwrap_or(0);
        let callee_line = callee_node.and_then(|n| n.line).unwrap_or(0);
        buf.push_str(&format!(
            "{caller},{callee},{},{},{},{},{},{}\n",
            sanitize_csv_field(caller_sym),
            sanitize_csv_field(callee_sym),
            sanitize_csv_field(caller_file),
            sanitize_csv_field(callee_file),
            caller_line,
            callee_line
        ));
    }
    fs::write(path, buf)?;
    Ok(())
}

pub fn emit_modulegraph_csv(
    out_dir: &Path,
    modulegraph: &[(u32, u32)],
    module_nodes: &[ModuleNode],
) -> Result<()> {
    let path = out_dir.join("modulegraph.csv");
    let mut writer = Writer::from_path(path)?;
    writer.write_record([
        "parent_module",
        "child_module",
        "parent_symbol",
        "child_symbol",
        "parent_file",
        "child_file",
    ])?;
    for (parent, child) in modulegraph {
        let parent_node = module_nodes.iter().find(|n| n.id == *parent);
        let child_node = module_nodes.iter().find(|n| n.id == *child);
        let parent_sym = parent_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let child_sym = child_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let parent_file = parent_node.map(|n| n.file.as_str()).unwrap_or("");
        let child_file = child_node.map(|n| n.file.as_str()).unwrap_or("");
        writer.write_record([
            parent_sym.to_string(),
            child_sym.to_string(),
            parent_sym.to_string(),
            child_sym.to_string(),
            parent_file.to_string(),
            child_file.to_string(),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

pub fn emit_nodes_csv(out_dir: &Path, nodes: &[NodeRow]) -> Result<()> {
    let path = out_dir.join("nodes.csv");
    let mut file = fs::File::create(path)?;
    writeln!(file, "node_id,node_kind,symbol,file_id,line,column,parent")?;
    for node in nodes {
        let symbol = sanitize_csv_field(&node.symbol);
        writeln!(
            file,
            "{},{},{},{},{},{},{}",
            node.id,
            node.kind,
            symbol,
            node.file_id.unwrap_or(0),
            node.line.unwrap_or(0),
            0,
            0
        )?;
    }
    Ok(())
}

pub fn emit_nodes_full_csv(
    out_dir: &Path,
    nodes: &[NodeRow],
    files: &[String],
) -> Result<()> {
    let path = out_dir.join("nodes_full.csv");
    let mut file = fs::File::create(path)?;
    writeln!(file, "node_id,node_kind,symbol,file_id,file_path,line")?;
    for node in nodes {
        let file_path = node
            .file_id
            .and_then(|id| files.get(id as usize))
            .cloned()
            .unwrap_or_default();
        let symbol = sanitize_csv_field(&node.symbol);
        writeln!(
            file,
            "{},{},{},{},{},{}",
            node.id,
            node.kind,
            symbol,
            node.file_id.unwrap_or(0),
            sanitize_csv_field(&file_path),
            node.line.unwrap_or(0),
        )?;
    }
    Ok(())
}

pub fn emit_nodes_raw_jsonl(
    out_dir: &Path,
    nodes: &[NodeRow],
    files: &[String],
) -> Result<()> {
    let path = out_dir.join("nodes_raw.jsonl");
    let mut file = fs::File::create(path)?;
    for node in nodes {
        let file_path = node
            .file_id
            .and_then(|id| files.get(id as usize))
            .cloned()
            .unwrap_or_default();
        let line = node.line.unwrap_or(0);
        let obj = serde_json::json!({
            "node_id": node.id,
            "kind": node.kind,
            "symbol": node.symbol,
            "file": file_path,
            "line": line,
        });
        writeln!(file, "{}", obj)?;
    }
    Ok(())
}

pub fn emit_edges_csv(out_dir: &Path, edges: &[EdgeRow]) -> Result<()> {
    let path = out_dir.join("edges.csv");
    let mut file = fs::File::create(path)?;
    writeln!(file, "src_id,dst_id,edge_kind")?;
    for edge in edges {
        writeln!(file, "{},{},{}", edge.src, edge.dst, edge.kind)?;
    }
    Ok(())
}

pub fn emit_edges_full_csv(
    out_dir: &Path,
    edges: &[EdgeRow],
    nodes: &[NodeRow],
    files: &[String],
) -> Result<()> {
    let path = out_dir.join("edges_full.csv");
    let mut file = fs::File::create(path)?;
    writeln!(file, "src_id,dst_id,edge_kind,src_symbol,dst_symbol,src_file,dst_file,src_line,dst_line")?;
    for edge in edges {
        let src_node = nodes.iter().find(|n| n.id == edge.src);
        let dst_node = nodes.iter().find(|n| n.id == edge.dst);
        let src_sym = src_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let dst_sym = dst_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let src_file = src_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let dst_file = dst_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let src_line = src_node.and_then(|n| n.line).unwrap_or(0);
        let dst_line = dst_node.and_then(|n| n.line).unwrap_or(0);
        writeln!(
            file,
            "{},{},{},{},{},{},{},{},{}",
            edge.src,
            edge.dst,
            edge.kind,
            sanitize_csv_field(src_sym),
            sanitize_csv_field(dst_sym),
            sanitize_csv_field(src_file),
            sanitize_csv_field(dst_file),
            src_line,
            dst_line
        )?;
    }
    Ok(())
}

pub fn emit_files_txt(out_dir: &Path, files: &[String]) -> Result<()> {
    let path = out_dir.join("files.txt");
    let mut file = fs::File::create(path)?;
    writeln!(file, "file_id,path")?;
    for (id, path_str) in files.iter().enumerate() {
        let sanitized = sanitize_csv_field(path_str);
        writeln!(file, "{},{}", id, sanitized)?;
    }
    Ok(())
}

pub fn emit_typegraph_csv(
    out_dir: &Path,
    typegraph: &[(u32, u32, String)],
    nodes: &[NodeRow],
    files: &[String],
) -> Result<()> {
    let path = out_dir.join("typegraph.csv");
    let mut buf = String::with_capacity(typegraph.len() * 80 + 64);
    buf.push_str("type_a,type_b,relation,type_a_symbol,type_b_symbol,type_a_file,type_b_file\n");
    for (a, b, rel) in typegraph {
        let a_node = nodes.iter().find(|n| n.id == *a);
        let b_node = nodes.iter().find(|n| n.id == *b);
        let a_sym = a_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let b_sym = b_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let a_file = a_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let b_file = b_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        buf.push_str(&format!("{a},{b},{rel},{a_sym},{b_sym},{a_file},{b_file}\n"));
    }
    fs::write(path, buf)?;
    Ok(())
}

pub fn emit_typegraph_full_csv(
    out_dir: &Path,
    typegraph: &[(u32, u32, String)],
    nodes: &[NodeRow],
    files: &[String],
) -> Result<()> {
    let path = out_dir.join("typegraph_full.csv");
    let mut buf = String::with_capacity(typegraph.len() * 90 + 64);
    buf.push_str("type_a,type_b,relation,type_a_symbol,type_b_symbol,type_a_file,type_b_file,type_a_line,type_b_line\n");
    for (a, b, rel) in typegraph {
        let a_node = nodes.iter().find(|n| n.id == *a);
        let b_node = nodes.iter().find(|n| n.id == *b);
        let a_sym = a_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let b_sym = b_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let a_file = a_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let b_file = b_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let a_line = a_node.and_then(|n| n.line).unwrap_or(0);
        let b_line = b_node.and_then(|n| n.line).unwrap_or(0);
        buf.push_str(&format!(
            "{a},{b},{rel},{},{},{},{},{},{}\n",
            sanitize_csv_field(a_sym),
            sanitize_csv_field(b_sym),
            sanitize_csv_field(a_file),
            sanitize_csv_field(b_file),
            a_line,
            b_line
        ));
    }
    fs::write(path, buf)?;
    Ok(())
}

fn sanitize_csv_field(raw: &str) -> String {
    let mut out = raw.replace('\n', " ").replace('\r', " ");
    if out.contains(',') {
        out = out.replace(',', ";");
    }
    out
}

pub fn emit_typegraph_csv_from_cache(
    out_dir: &Path,
    typegraph: &[(u32, u32, String)],
    nodes: &[TypeNodeRow],
) -> Result<()> {
    let path = out_dir.join("typegraph.csv");
    let mut buf = String::with_capacity(typegraph.len() * 80 + 64);
    buf.push_str("type_a,type_b,relation,type_a_symbol,type_b_symbol,type_a_file,type_b_file\n");
    for (a, b, rel) in typegraph {
        let a_node = nodes.iter().find(|n| n.id == *a);
        let b_node = nodes.iter().find(|n| n.id == *b);
        let a_sym = a_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let b_sym = b_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let a_file = a_node.map(|n| n.file.as_str()).unwrap_or("");
        let b_file = b_node.map(|n| n.file.as_str()).unwrap_or("");
        buf.push_str(&format!("{a},{b},{rel},{a_sym},{b_sym},{a_file},{b_file}\n"));
    }
    fs::write(path, buf)?;
    Ok(())
}

pub fn build_modulegraph(
    nodes: &[NodeRow],
    files: &[String],
) -> (Vec<(u32, u32)>, Vec<ModuleNode>) {
    let mut module_files: BTreeMap<String, String> = BTreeMap::new();
    for node in nodes {
        if node.kind != "MODULE" {
            continue;
        }
        let file = node
            .file_id
            .and_then(|id| files.get(id as usize))
            .cloned()
            .unwrap_or_default();
        module_files.insert(node.symbol.clone(), file);
    }
    let base_symbols: Vec<(String, String)> = module_files.iter().map(|(s, f)| (s.clone(), f.clone())).collect();
    for (sym, file) in base_symbols {
        for prefix in module_prefixes(&sym) {
            module_files.entry(prefix).or_insert_with(|| file.clone());
        }
    }
    if !module_files.contains_key("") {
        module_files.insert("".to_string(), String::new());
    }

    let mut module_nodes: Vec<ModuleNode> = Vec::new();
    let mut symbol_to_id: BTreeMap<String, u32> = BTreeMap::new();
    let mut next_id: u32 = 0;

    for (symbol, file) in module_files.iter() {
        let id = next_id;
        next_id += 1;
        symbol_to_id.insert(symbol.clone(), id);
        module_nodes.push(ModuleNode {
            id,
            symbol: if symbol.is_empty() { "crate".to_string() } else { symbol.clone() },
            file: file.clone(),
        });
    }

    let mut edges: BTreeSet<(u32, u32)> = BTreeSet::new();
    for symbol in module_files.keys() {
        if symbol.is_empty() {
            continue;
        }
        let parent_symbol = match symbol.rsplit_once("::") {
            Some((parent, _child)) => parent,
            None => "",
        };
        let Some(&parent_id) = symbol_to_id.get(parent_symbol) else { continue };
        let Some(&child_id) = symbol_to_id.get(symbol) else { continue };
        edges.insert((parent_id, child_id));
    }

    (edges.into_iter().collect(), module_nodes)
}

#[derive(Debug, Clone)]
pub struct TypeNodeRow {
    id: u32,
    symbol: String,
    file: String,
}

pub fn build_modulegraph_from_cache(cache: &GraphCache) -> (Vec<(u32, u32)>, Vec<ModuleNode>) {
    let mut module_files = cache.module_files.clone();
    if !module_files.contains_key("") {
        module_files.insert("".to_string(), String::new());
    }

    let mut module_nodes: Vec<ModuleNode> = Vec::new();
    let mut symbol_to_id: BTreeMap<String, u32> = BTreeMap::new();
    let mut next_id: u32 = 0;

    for (symbol, file) in module_files.iter() {
        let id = next_id;
        next_id += 1;
        symbol_to_id.insert(symbol.clone(), id);
        module_nodes.push(ModuleNode {
            id,
            symbol: if symbol.is_empty() { "crate".to_string() } else { symbol.clone() },
            file: file.clone(),
        });
    }

    let mut edges: BTreeSet<(u32, u32)> = BTreeSet::new();
    for symbol in module_files.keys() {
        if symbol.is_empty() {
            continue;
        }
        let parent_symbol = match symbol.rsplit_once("::") {
            Some((parent, _child)) => parent,
            None => "",
        };
        let Some(&parent_id) = symbol_to_id.get(parent_symbol) else { continue };
        let Some(&child_id) = symbol_to_id.get(symbol) else { continue };
        edges.insert((parent_id, child_id));
    }

    (edges.into_iter().collect(), module_nodes)
}

pub fn build_typegraph_edges(nodes: &[NodeRow], edges: &[EdgeRow]) -> Vec<(u32, u32, String)> {
    let id_to_kind: HashMap<u32, &str> = nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let type_kinds = ["STRUCT", "ENUM", "TRAIT", "IMPL", "TYPE"];
    let rel_kinds = ["HAS_FIELD", "HAS_METHOD", "IMPLEMENTS", "FOR_TYPE", "USES_TYPE", "BOUNDS"];
    let mut seen: BTreeSet<(u32, u32, String)> = BTreeSet::new();

    for edge in edges {
        if !rel_kinds.contains(&edge.kind.as_str()) {
            continue;
        }
        let src_kind = id_to_kind.get(&edge.src);
        let dst_kind = id_to_kind.get(&edge.dst);
        let src_ok = src_kind.map(|k| type_kinds.contains(k)).unwrap_or(false);
        let dst_ok = dst_kind.map(|k| type_kinds.contains(k)).unwrap_or(false);
        if src_ok && dst_ok {
            seen.insert((edge.src, edge.dst, edge.kind.clone()));
        }
    }

    if seen.is_empty() {
        for node in nodes {
            if type_kinds.contains(&node.kind.as_str()) {
                seen.insert((node.id, node.id, "DECL".to_string()));
            }
        }
    }

    seen.into_iter().collect()
}

pub fn build_typegraph_from_cache(cache: &GraphCache) -> (Vec<(u32, u32, String)>, Vec<TypeNodeRow>) {
    let mut nodes: Vec<TypeNodeRow> = Vec::new();
    let mut symbol_to_id: BTreeMap<String, u32> = BTreeMap::new();
    let mut next_id: u32 = 0;

    for (symbol, node) in cache.type_nodes.iter() {
        let id = next_id;
        next_id += 1;
        symbol_to_id.insert(symbol.clone(), id);
        nodes.push(TypeNodeRow {
            id,
            symbol: symbol.clone(),
            file: node.file.clone(),
        });
    }

    let mut edges: BTreeSet<(u32, u32, String)> = BTreeSet::new();
    for edge in cache.type_edges.iter() {
        let src_id = match symbol_to_id.get(&edge.src) {
            Some(id) => *id,
            None => {
                let id = next_id;
                next_id += 1;
                symbol_to_id.insert(edge.src.clone(), id);
                nodes.push(TypeNodeRow {
                    id,
                    symbol: edge.src.clone(),
                    file: String::new(),
                });
                id
            }
        };
        let dst_id = match symbol_to_id.get(&edge.dst) {
            Some(id) => *id,
            None => {
                let id = next_id;
                next_id += 1;
                symbol_to_id.insert(edge.dst.clone(), id);
                nodes.push(TypeNodeRow {
                    id,
                    symbol: edge.dst.clone(),
                    file: String::new(),
                });
                id
            }
        };
        edges.insert((src_id, dst_id, edge.rel.clone()));
    }

    if edges.is_empty() {
        for node in &nodes {
            edges.insert((node.id, node.id, "DECL".to_string()));
        }
    }

    (edges.into_iter().collect(), nodes)
}

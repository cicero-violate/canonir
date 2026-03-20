use anyhow::{anyhow, Result};
use memmap2::Mmap;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// Re-export CodeGraphEdge from the event-store layer; it has the same shape.
pub use canon_event_store::CodeGraphEdge as Edge;

#[derive(Debug, Clone)]
pub struct Node {
    pub id: u32,
    pub kind: String,
    pub symbol: String,
    pub file: String,
    pub line: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct CsrGraph {
    pub row_ptr: Vec<u32>,
    pub col_idx: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct CodeGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub adjacency: CsrGraph,
    pub symbol_to_id: HashMap<String, u32>,
    pub files: Vec<String>,
}

pub fn load_code_graph(graph_dir: &Path) -> Result<CodeGraph> {
    let graph_bin = graph_dir.join("graph.bin");
    if !graph_bin.exists() {
        return Err(anyhow!("graph.bin not found at {}", graph_bin.display()));
    }
    let (nodes, edges, files) = load_graph_bin(&graph_bin)?;
    let symbol_to_id = nodes.iter().filter(|n| !n.symbol.is_empty()).map(|n| (n.symbol.clone(), n.id)).collect::<HashMap<_, _>>();
    let adjacency = build_csr(nodes.len(), &edges);
    Ok(CodeGraph { nodes, edges, adjacency, symbol_to_id, files })
}

fn build_csr(node_count: usize, edges: &[Edge]) -> CsrGraph {
    let mut row_ptr = vec![0u32; node_count + 1];
    for e in edges {
        if (e.src as usize) < node_count {
            row_ptr[e.src as usize + 1] += 1;
        }
    }
    for i in 1..row_ptr.len() {
        row_ptr[i] += row_ptr[i - 1];
    }
    let mut col_idx = vec![0u32; edges.len()];
    let mut cursor = row_ptr.clone();
    for e in edges {
        let src = e.src as usize;
        if src >= node_count {
            continue;
        }
        let pos = cursor[src] as usize;
        if pos < col_idx.len() {
            col_idx[pos] = e.dst;
            cursor[src] += 1;
        }
    }
    CsrGraph { row_ptr, col_idx }
}

fn load_graph_bin(path: &Path) -> Result<(Vec<Node>, Vec<Edge>, Vec<String>)> {
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

    let mut nodes: Vec<Node> = Vec::with_capacity(n_nodes);
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
                std::str::from_utf8(&string_table[sym_off..end]).unwrap_or("").to_string()
            } else {
                String::new()
            }
        };

        let file = if file_id == NO_FILE_ID { String::new() } else { files.get(file_id as usize).cloned().unwrap_or_default() };

        nodes.push(Node { id, kind: node_kind_str(kind_code).to_string(), symbol, file, line: if line == NO_LINE { None } else { Some(line) } });
    }

    let mut edges: Vec<Edge> = Vec::with_capacity(n_edges);
    let mut pos = edges_offset;
    for _ in 0..n_edges {
        let src = u32::from_le_bytes(data[pos..pos + 4].try_into()?);
        let dst = u32::from_le_bytes(data[pos + 4..pos + 8].try_into()?);
        let kind_code = data[pos + 8];
        pos += EDGE_RECORD_SIZE;
        edges.push(Edge { src, dst, kind: edge_kind_str(kind_code).to_string() });
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

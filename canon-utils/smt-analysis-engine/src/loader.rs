use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
pub use canon_types::{
    edge_kind_str,
    node_kind_str,
    parse_edge_kind,
    parse_node_kind,
    Edge,
    EdgeKind,
    Metadata,
    Node,
    NodeKind,
    SCHEMA_VERSION,
};


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
    pub id_to_index: HashMap<u32, usize>,
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

    let id_to_index: HashMap<u32, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
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
        id_to_index,
    })
}

fn read_nodes_csv(path: PathBuf) -> Result<Vec<Node>> {
    let content = fs::read_to_string(&path)?;
    let files = read_files_txt(path.parent().unwrap_or_else(|| Path::new(".")).join("files.txt"))?;
    let mut nodes = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 7 {
            return Err(anyhow!("invalid nodes.csv line"));
        }
        let id = match parts[0].parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let kind = match parse_node_kind(parts[1]) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let line_no = match parts[parts.len() - 3].parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let col = match parts[parts.len() - 2].parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let file_id = match parts[parts.len() - 4].parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let parent = match parts[parts.len() - 1].parse::<u32>() {
            Ok(v) => v,
            Err(_) => 0,
        };
        let file = files.get(file_id as usize).cloned().unwrap_or_default();
        let symbol = parts[2..parts.len() - 4].join(",");
        nodes.push(Node {
            id,
            kind,
            symbol,
            file,
            line: line_no,
            column: col,
            file_id: Some(file_id),
            parent: Some(parent),
        });
    }
    Ok(nodes)
}

fn read_files_txt(path: PathBuf) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    let mut files = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 2 {
            continue;
        }
        let id = match parts[0].parse::<usize>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let path = parts[1..].join(",");
        if files.len() <= id {
            files.resize(id + 1, String::new());
        }
        files[id] = path;
    }
    Ok(files)
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
        let src = match parts[0].parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let dst = match parts[1].parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let kind = match parse_edge_kind(parts[2]) {
            Ok(v) => v,
            Err(_) => continue,
        };
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
    let meta: Metadata = serde_json::from_str(&content)
        .map_err(|e| anyhow!("metadata.json parse error: {e}"))?;
    if meta.schema_version != SCHEMA_VERSION {
        return Err(anyhow!(
            "schema version mismatch: expected {}, found {} — re-run the UPG extractor",
            SCHEMA_VERSION,
            meta.schema_version
        ));
    }
    Ok(meta)
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

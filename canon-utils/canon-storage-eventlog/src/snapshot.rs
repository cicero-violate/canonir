use anyhow::{anyhow, Result};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize, Infallible};
use rkyv::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::graph_types::{CodeGraphEdge, CodeGraphNode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub tlog_offset: u64,
    pub event_count: u64,
    pub created_at: u64,
    #[serde(default)]
    pub version: u32,
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct CodeSnapshot {
    pub nodes: Vec<CodeSnapshotNode>,
    pub edges: Vec<CodeSnapshotEdge>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct CodeSnapshotNode {
    pub kind: String,
    pub symbol: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct CodeSnapshotEdge {
    pub src_symbol: String,
    pub src_kind: String,
    pub dst_symbol: String,
    pub dst_kind: String,
    pub kind: String,
}

pub fn read_snapshot_metadata(path: &Path) -> Result<SnapshotMeta> {
    let data = fs::read_to_string(path)?;
    let meta = serde_json::from_str(&data)?;
    Ok(meta)
}

pub fn write_snapshot_metadata(path: &Path, meta: &SnapshotMeta) -> Result<()> {
    let data = serde_json::to_string_pretty(meta)?;
    fs::write(path, data)?;
    Ok(())
}

pub fn load_graph_snapshot(path: &Path) -> Result<CodeSnapshot> {
    let data = fs::read(path)?;
    let archived = std::panic::catch_unwind(|| unsafe { rkyv::archived_root::<CodeSnapshot>(&data) });
    let archived = match archived {
        Ok(v) => v,
        Err(_) => {
            return Err(anyhow!("snapshot deserialize failed: panic during archived_root"));
        }
    };
    let snapshot: CodeSnapshot = archived
        .deserialize(&mut Infallible)
        .map_err(|e| anyhow!("snapshot deserialize failed: {e}"))?;
    Ok(snapshot)
}

pub fn save_graph_snapshot(
    path: &Path,
    nodes: &[CodeGraphNode],
    edges: &[CodeGraphEdge],
    files: &[String],
) -> Result<()> {
    let mut nodes_out: Vec<CodeSnapshotNode> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let file = node
            .file_id
            .and_then(|id| files.get(id as usize))
            .cloned()
            .unwrap_or_default();
        nodes_out.push(CodeSnapshotNode {
            kind: node.kind.clone(),
            symbol: node.symbol.clone(),
            file,
            line: node.line.unwrap_or(0),
            column: 0,
        });
    }

    let mut id_to_kind: HashMap<u32, (&str, &str)> = HashMap::new();
    for node in nodes {
        id_to_kind.insert(node.id, (node.symbol.as_str(), node.kind.as_str()));
    }

    let mut edges_out: Vec<CodeSnapshotEdge> = Vec::with_capacity(edges.len());
    for edge in edges {
        let (src_sym, src_kind) = id_to_kind
            .get(&edge.src)
            .copied()
            .unwrap_or(("", "UNKNOWN"));
        let (dst_sym, dst_kind) = id_to_kind
            .get(&edge.dst)
            .copied()
            .unwrap_or(("", "UNKNOWN"));
        edges_out.push(CodeSnapshotEdge {
            src_symbol: src_sym.to_string(),
            src_kind: src_kind.to_string(),
            dst_symbol: dst_sym.to_string(),
            dst_kind: dst_kind.to_string(),
            kind: edge.kind.clone(),
        });
    }

    let snapshot = CodeSnapshot {
        nodes: nodes_out,
        edges: edges_out,
        files: files.to_vec(),
    };

    if estimate_snapshot_size(&snapshot) > i32::MAX as u64 {
        eprintln!(
            "canon_reports: kernel snapshot too large for rkyv (>{} bytes), skipping write to {}",
            i32::MAX,
            path.display()
        );
        return Ok(());
    }

    let serialize_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut serializer = rkyv::ser::serializers::AllocSerializer::<256>::default();
        serializer
            .serialize_value(&snapshot)
            .map_err(|e| anyhow!("snapshot serialize failed: {e}"))?;
        let buf = serializer.into_serializer().into_inner();
        fs::write(path, buf)?;
        Ok::<(), anyhow::Error>(())
    }));

    match serialize_result {
        Ok(res) => res?,
        Err(_) => {
            eprintln!(
                "canon_reports: kernel snapshot serialization panicked (rkyv ExceedsStorageRange likely). Skipping write to {}",
                path.display()
            );
        }
    }
    Ok(())
}

pub fn estimate_snapshot_size(snapshot: &CodeSnapshot) -> u64 {
    let mut total = 0u64;
    for n in &snapshot.nodes {
        total = total.saturating_add(n.kind.len() as u64);
        total = total.saturating_add(n.symbol.len() as u64);
        total = total.saturating_add(n.file.len() as u64);
        total = total.saturating_add(64);
    }
    for e in &snapshot.edges {
        total = total.saturating_add(e.src_symbol.len() as u64);
        total = total.saturating_add(e.src_kind.len() as u64);
        total = total.saturating_add(e.dst_symbol.len() as u64);
        total = total.saturating_add(e.dst_kind.len() as u64);
        total = total.saturating_add(e.kind.len() as u64);
        total = total.saturating_add(64);
    }
    for f in &snapshot.files {
        total = total.saturating_add(f.len() as u64);
        total = total.saturating_add(16);
    }
    total
}

pub fn snapshot_into_rows(
    snapshot: CodeSnapshot,
) -> (Vec<CodeGraphNode>, Vec<CodeGraphEdge>, Vec<String>) {
    let mut files = snapshot.files;
    let mut file_map: HashMap<String, u32> = HashMap::new();
    for (idx, path) in files.iter().enumerate() {
        file_map.insert(path.clone(), idx as u32);
    }

    let mut nodes: Vec<CodeGraphNode> = Vec::new();
    let mut key_to_id: HashMap<(String, String), u32> = HashMap::new();
    for node in snapshot.nodes {
        let file_id = if node.file.is_empty() {
            None
        } else if let Some(id) = file_map.get(&node.file).copied() {
            Some(id)
        } else {
            files.push(node.file.clone());
            let id = (files.len() - 1) as u32;
            file_map.insert(node.file.clone(), id);
            Some(id)
        };
        let id = nodes.len() as u32;
        key_to_id.insert((node.symbol.clone(), node.kind.clone()), id);
        nodes.push(CodeGraphNode {
            id,
            kind: node.kind,
            symbol: node.symbol,
            file_id,
            line: Some(node.line).filter(|v| *v > 0),
        });
    }

    let mut edges: Vec<CodeGraphEdge> = Vec::new();
    for edge in snapshot.edges {
        let Some(&src) = key_to_id.get(&(edge.src_symbol, edge.src_kind)) else {
            continue;
        };
        let Some(&dst) = key_to_id.get(&(edge.dst_symbol, edge.dst_kind)) else {
            continue;
        };
        edges.push(CodeGraphEdge {
            src,
            dst,
            kind: edge.kind,
        });
    }

    (nodes, edges, files)
}

use serde_json::Value;
use std::collections::HashMap;

use crate::artifacts_loader::{CsrGraph, Edge as GraphEdge, KernelGraph as LoadedGraph, Node as GraphNode};
use crate::graph::csr::build_csr_graph;
use crate::graph::graph_types::{EdgeRow, NodeRow};

pub fn apply_event_to_graph(
    value: Value,
    nodes: &mut Vec<NodeRow>,
    edges: &mut Vec<EdgeRow>,
    files: &mut Vec<String>,
    symbol_to_id: &mut HashMap<String, u32>,
    clear_on_session: bool,
) -> bool {
    let Some(tag) = value.get("t").and_then(|v| v.as_str()) else {
        return false;
    };
    match tag {
        "SESSION" => {
            if clear_on_session {
                nodes.clear();
                edges.clear();
                files.clear();
                symbol_to_id.clear();
            }
            true
        }
        "N" | "NODE" | "NODE_UPDATE" => {
            let sym = value.get("sym").and_then(|v| v.as_str()).unwrap_or("");
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let file = value.get("file").and_then(|v| v.as_str()).unwrap_or("");
            let line = value.get("line").and_then(|v| v.as_u64()).map(|v| v as u32);
            if kind.is_empty() {
                return false;
            }
            if sym.is_empty() && kind != "MODULE" {
                return false;
            }
            if !sym.is_empty() {
                if let Some(&id) = symbol_to_id.get(sym) {
                    if let Some(node) = nodes.get_mut(id as usize) {
                        node.kind = kind.to_string();
                        if !file.is_empty() {
                            let file_id = files.iter().position(|p| p == file).map(|idx| idx as u32).or_else(|| {
                                files.push(file.to_string());
                                Some((files.len() - 1) as u32)
                            });
                            node.file_id = file_id;
                        }
                        if line.is_some() {
                            node.line = line;
                        }
                        return true;
                    }
                }
            }
            let file_id = if file.is_empty() {
                None
            } else {
                let file_id = files.iter().position(|p| p == file).map(|idx| idx as u32);
                file_id.or_else(|| {
                    files.push(file.to_string());
                    Some((files.len() - 1) as u32)
                })
            };
            let id = nodes.len() as u32;
            nodes.push(NodeRow {
                id,
                kind: kind.to_string(),
                symbol: sym.to_string(),
                file_id,
                line,
            });
            if !sym.is_empty() {
                symbol_to_id.insert(sym.to_string(), id);
            }
            true
        }
        "E" | "EDGE" => {
            let src_sym = value.get("src").and_then(|v| v.as_str()).unwrap_or("");
            let dst_sym = value.get("dst").and_then(|v| v.as_str()).unwrap_or("");
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let Some(&src) = symbol_to_id.get(src_sym) else {
                return false;
            };
            let Some(&dst) = symbol_to_id.get(dst_sym) else {
                return false;
            };
            edges.push(EdgeRow {
                src,
                dst,
                kind: kind.to_string(),
            });
            true
        }
        "NODE_REMOVE" => {
            let sym = value.get("sym").and_then(|v| v.as_str()).unwrap_or("");
            let Some(&id) = symbol_to_id.get(sym) else {
                return false;
            };
            delete_node(id, nodes, edges, symbol_to_id)
        }
        "EDGE_REMOVE" => {
            let src_sym = value.get("src").and_then(|v| v.as_str()).unwrap_or("");
            let dst_sym = value.get("dst").and_then(|v| v.as_str()).unwrap_or("");
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let Some(&src) = symbol_to_id.get(src_sym) else {
                return false;
            };
            let Some(&dst) = symbol_to_id.get(dst_sym) else {
                return false;
            };
            let before = edges.len();
            edges.retain(|e| !(e.src == src && e.dst == dst && e.kind == kind));
            before != edges.len()
        }
        "F" | "FILE" => {
            let path = value.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if !path.is_empty() && !files.iter().any(|p| p == path) {
                files.push(path.to_string());
            }
            true
        }
        "PANIC" | "WARNING" => {
            // telemetry-only events; do not affect graph reconstruction
            true
        }
        _ => false,
    }
}

pub fn delete_node(
    id: u32,
    nodes: &mut Vec<NodeRow>,
    edges: &mut Vec<EdgeRow>,
    symbol_to_id: &mut HashMap<String, u32>,
) -> bool {
    let idx = id as usize;
    if idx >= nodes.len() {
        return false;
    }
    let last_idx = nodes.len() - 1;
    let removed = nodes.swap_remove(idx);
    if !removed.symbol.is_empty() {
        symbol_to_id.remove(&removed.symbol);
    }
    edges.retain(|e| e.src != id && e.dst != id);
    if idx != last_idx {
        let swapped_id = id;
        let old_last_id = last_idx as u32;
        if let Some(node) = nodes.get_mut(idx) {
            node.id = swapped_id;
            if !node.symbol.is_empty() {
                symbol_to_id.insert(node.symbol.clone(), swapped_id);
            }
        }
        for e in edges.iter_mut() {
            if e.src == old_last_id {
                e.src = swapped_id;
            }
            if e.dst == old_last_id {
                e.dst = swapped_id;
            }
        }
    }
    true
}
 

pub fn rows_to_kernel_graph(
    nodes: &[NodeRow],
    edges: &[EdgeRow],
    files: &[String],
) -> LoadedGraph {
    let mut out_nodes = Vec::with_capacity(nodes.len());
    for n in nodes {
        let file = n
            .file_id
            .and_then(|id| files.get(id as usize))
            .cloned()
            .unwrap_or_default();
        out_nodes.push(GraphNode {
            id: n.id,
            kind: n.kind.clone(),
            symbol: n.symbol.clone(),
            file,
            line: n.line,
        });
    }
    let out_edges = edges
        .iter()
        .map(|e| GraphEdge {
            src: e.src,
            dst: e.dst,
            kind: e.kind.clone(),
        })
        .collect::<Vec<_>>();
    let symbol_to_id = out_nodes
        .iter()
        .filter(|n| !n.symbol.is_empty())
        .map(|n| (n.symbol.clone(), n.id))
        .collect::<HashMap<_, _>>();
    let adjacency = build_csr_graph(out_nodes.len(), &out_edges);
    LoadedGraph {
        nodes: out_nodes,
        edges: out_edges,
        adjacency,
        symbol_to_id,
        files: files.to_vec(),
    }
}

pub fn rebuild_symbol_index(nodes: &[NodeRow]) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for node in nodes {
        map.insert(node.symbol.clone(), node.id);
    }
    map
}

pub 

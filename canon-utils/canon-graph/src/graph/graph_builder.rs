use std::collections::HashMap;

use crate::artifacts_loader::{CodeGraph as LoadedGraph, Node as GraphNode};
use crate::graph::csr::build_csr_graph;
use crate::graph::graph_types::{CodeEdge, CodeNode};

pub fn rows_to_code_graph(
    nodes: &[CodeNode],
    edges: &[CodeEdge],
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
    let out_edges = edges.to_vec();
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

pub fn module_prefixes(sym: &str) -> Vec<String> {
    let parts: Vec<&str> = sym.split("::").collect();
    if parts.len() <= 1 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(parts.len().saturating_sub(1));
    let mut current = Vec::new();
    for part in parts.iter().take(parts.len() - 1) {
        current.push(*part);
        out.push(current.join("::"));
    }
    out
}

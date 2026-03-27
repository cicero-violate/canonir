use crate::symbol_index::SymbolIndex;
use anyhow::{anyhow, Result};
use canon_analysis::load_latest_workspace_graph_artifact;
use canon_ir::{CanonNodeKind, NodeId};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct GraphEdgeRecord {
    pub src: String,
    pub dst: String,
    pub kind: String,
}

pub struct AnalysisSession {
    pub module_files: HashMap<String, PathBuf>,
    pub file_modules: HashMap<PathBuf, Vec<String>>,
    pub files: HashSet<PathBuf>,
    pub uses_crate_prefix: bool,
    edges: Vec<GraphEdgeRecord>,
}

impl AnalysisSession {
    pub fn load(project_root: &Path) -> Result<Self> {
        let index = SymbolIndex::build(project_root)?;
        if index.module_files().is_empty() {
            return Err(anyhow!("analysis has no module mapping for {}", project_root.display()));
        }
        let (_summary, ir) = load_latest_workspace_graph_artifact(project_root)?;
        let node_symbols = graph_symbol_paths(&ir);
        let mut edges = Vec::new();
        edges.extend(graph_edges_from_csr(&ir.call_graph, "CALL", &node_symbols));
        edges.extend(graph_edges_from_csr(&ir.module_graph, "MODULE", &node_symbols));
        edges.extend(graph_edges_from_csr(&ir.cfg_graph, "CFG", &node_symbols));
        Ok(Self {
            module_files: index.module_files().clone(),
            file_modules: index.file_modules().clone(),
            files: index.files().clone(),
            uses_crate_prefix: index.uses_crate_prefix(),
            edges,
        })
    }

    pub fn edges_by_kind(&self, edge_kind: &str) -> Result<Vec<GraphEdgeRecord>> {
        Ok(self.edges.iter().filter(|e| e.kind == edge_kind).cloned().collect())
    }

    pub fn callers_of(&self, sym: &str) -> Result<Vec<GraphEdgeRecord>> {
        Ok(self.edges.iter().filter(|e| e.kind == "CALL" && e.dst == sym).cloned().collect())
    }
}

fn graph_symbol_paths(ir: &canon_ir::CanonIR) -> HashMap<u32, String> {
    let module_membership = module_membership(ir);
    let mut out = HashMap::new();
    for node in &ir.nodes {
        let path = match &node.kind {
            CanonNodeKind::Module { path_id, .. } => ir.lookup_path(*path_id).to_string(),
            CanonNodeKind::Struct { name_id, .. }
            | CanonNodeKind::Enum { name_id, .. }
            | CanonNodeKind::Trait { name_id, .. }
            | CanonNodeKind::AssocType { name_id, .. }
            | CanonNodeKind::AssocConst { name_id, .. }
            | CanonNodeKind::Fn { name_id, .. } => {
                let module_path = module_membership
                    .get(&node.id.0)
                    .cloned()
                    .unwrap_or_else(|| "crate".to_string());
                format!("{}::{}", module_path, ir.lookup_name(*name_id))
            }
            _ => continue,
        };
        out.insert(node.id.0, path);
    }
    out
}

fn module_membership(ir: &canon_ir::CanonIR) -> HashMap<u32, String> {
    let mut membership = HashMap::new();
    for node in &ir.nodes {
        let CanonNodeKind::Module { path_id, .. } = &node.kind else {
            continue;
        };
        let module_path = ir.lookup_path(*path_id).to_string();
        for (dst, _) in ir.module_graph.neighbours(NodeId(node.id.0)) {
            membership.entry(dst.0).or_insert_with(|| module_path.clone());
        }
    }
    membership
}

fn graph_edges_from_csr(
    graph: &canon_ir::csr_graph::CsrGraph<canon_ir::CanonId, canon_ir::EdgeKind>,
    kind: &str,
    node_symbols: &HashMap<u32, String>,
) -> Vec<GraphEdgeRecord> {
    let mut edges = Vec::new();
    for src in 0..graph.vertex_count() {
        for (dst, _) in graph.neighbours(NodeId(src as u32)) {
            let Some(src_symbol) = node_symbols.get(&(src as u32)) else {
                continue;
            };
            let Some(dst_symbol) = node_symbols.get(&dst.0) else {
                continue;
            };
            edges.push(GraphEdgeRecord {
                src: src_symbol.clone(),
                dst: dst_symbol.clone(),
                kind: kind.to_string(),
            });
        }
    }
    edges
}

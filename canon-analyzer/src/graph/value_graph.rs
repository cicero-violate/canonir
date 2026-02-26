use canon::node::{CanonId, CanonNodeKind};
use canon::CanonIR;
use model::ir::{csr_graph::CsrGraph, edge::EdgeKind};
use std::collections::HashMap;

pub struct ValueGraphBuilder {
    v: usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl ValueGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_const_dep(&mut self, src: CanonId, dst: CanonId) {
        self.edges.push((src.0, dst.0, EdgeKind::ConstDep));
    }

    pub fn derive_from_ir(&mut self, ir: &CanonIR) {
        let consts: HashMap<String, CanonId> = ir
            .nodes
            .iter()
            .filter_map(|n| match &n.kind {
                CanonNodeKind::Const { name_id, .. } | CanonNodeKind::Static { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), n.id)),
                _ => None,
            })
            .collect();

        for n in &ir.nodes {
            let value = match &n.kind {
                CanonNodeKind::Const { value_id, .. } | CanonNodeKind::Static { value_id, .. } => ir.lookup_name(*value_id),
                _ => continue,
            };
            for (name, dep_id) in &consts {
                if *dep_id == n.id {
                    continue;
                }
                if contains_ident(value, name) {
                    self.add_const_dep(n.id, *dep_id);
                }
            }
        }
    }

    pub fn edges(&self) -> &[(u32, u32, EdgeKind)] {
        &self.edges
    }

    pub fn build(self) -> CsrGraph<CanonId, EdgeKind> {
        let node_ids: Vec<CanonId> = (0..self.v as u32).map(CanonId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}

fn contains_ident(text: &str, ident: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|tok| tok == ident)
}

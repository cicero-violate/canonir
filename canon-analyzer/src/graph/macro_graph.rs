use canon::node::{CanonId, CanonNodeKind};
use canon::CanonIR;
use model::ir::{csr_graph::CsrGraph, edge::EdgeKind};

pub struct MacroGraphBuilder {
    v: usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl MacroGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_expands(&mut self, src: CanonId, dst: CanonId) {
        self.edges.push((src.0, dst.0, EdgeKind::Expands));
    }

    pub fn derive_from_ir(&mut self, ir: &CanonIR) {
        let mut module_children: Vec<Vec<usize>> = vec![Vec::new(); ir.nodes.len()];
        for src in 0..ir.module_graph.vertex_count() {
            let src_id = model::ir::node::NodeId(src as u32);
            for (dst, edge) in ir.module_graph.neighbours(src_id) {
                if *edge == EdgeKind::Contains {
                    module_children[src].push(dst.index());
                }
            }
        }

        for (mid, children) in module_children.iter().enumerate() {
            let macros: Vec<CanonId> = children
                .iter()
                .filter_map(|&idx| match ir.nodes.get(idx).map(|n| &n.kind) {
                    Some(CanonNodeKind::MacroCall { .. }) => Some(CanonId(idx as u32)),
                    _ => None,
                })
                .collect();

            if macros.is_empty() {
                continue;
            }

            let expand_targets: Vec<CanonId> = children
                .iter()
                .filter_map(|&idx| match ir.nodes.get(idx).map(|n| &n.kind) {
                    Some(CanonNodeKind::MacroCall { .. }) | Some(CanonNodeKind::Module { .. }) | Some(CanonNodeKind::Crate { .. }) => None,
                    Some(_) => Some(CanonId(idx as u32)),
                    None => None,
                })
                .collect();

            let _ = mid;
            for m in &macros {
                for t in &expand_targets {
                    self.add_expands(*m, *t);
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

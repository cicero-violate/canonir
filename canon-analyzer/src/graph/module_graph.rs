use canon::csr_graph::CsrGraph;
use canon::edge::EdgeKind;
use canon::node::{CanonId, CanonNodeKind};
use canon::CanonIR;
use std::collections::HashMap;

pub struct ModuleGraphBuilder {
    v: usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl ModuleGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_contains(&mut self, parent: CanonId, child: CanonId) {
        self.edges.push((parent.0, child.0, EdgeKind::Contains));
    }

    pub fn add_impl_for(&mut self, impl_node: CanonId, target: CanonId) {
        self.edges.push((impl_node.0, target.0, EdgeKind::ImplFor));
    }

    pub fn derive_from_ir(&mut self, ir: &CanonIR) {
        let crate_id = ir.nodes.iter().find_map(|n| if matches!(n.kind, CanonNodeKind::Crate { .. }) { Some(n.id) } else { None });

        let mut module_by_path: HashMap<String, CanonId> = HashMap::new();
        for n in &ir.nodes {
            if let CanonNodeKind::Module { path_id, .. } = &n.kind {
                module_by_path.insert(ir.lookup_path(*path_id).to_string(), n.id);
            }
        }

        for n in &ir.nodes {
            if let CanonNodeKind::Module { path_id, .. } = &n.kind {
                let path = ir.lookup_path(*path_id);
                let parent_path = path.rsplit_once("::").map(|(p, _)| p.to_string());
                if let Some(parent) = parent_path.and_then(|p| module_by_path.get(&p).copied()) {
                    self.add_contains(parent, n.id);
                } else if let Some(cid) = crate_id {
                    self.add_contains(cid, n.id);
                }
            }

            match &n.kind {
                CanonNodeKind::Trait { methods, .. } => {
                    for m in methods {
                        self.add_contains(n.id, *m);
                    }
                }
                CanonNodeKind::Struct { fields, .. } => {
                    for f in fields {
                        self.add_contains(n.id, *f);
                    }
                }
                CanonNodeKind::Enum { variants, .. } => {
                    for v in variants {
                        self.add_contains(n.id, *v);
                    }
                }
                CanonNodeKind::Body { blocks } => {
                    for bb in blocks {
                        self.add_contains(n.id, *bb);
                    }
                }
                CanonNodeKind::Impl { for_ty, .. } => {
                    self.add_impl_for(n.id, *for_ty);
                }
                _ => {}
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

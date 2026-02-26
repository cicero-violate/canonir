use canon::csr_graph::CsrGraph;
use canon::edge::EdgeKind;
use canon::node::{CanonId, CanonNodeKind};
use canon::CanonIR;

pub struct RegionGraphBuilder {
    v: usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl RegionGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_outlives(&mut self, a: CanonId, b: CanonId) {
        self.edges.push((a.0, b.0, EdgeKind::Outlives));
    }

    pub fn derive_from_ir(&mut self, ir: &CanonIR) {
        for n in &ir.nodes {
            if let CanonNodeKind::GenericParam { bounds, is_lifetime, .. } = &n.kind {
                if !is_lifetime {
                    continue;
                }
                for b in bounds {
                    if matches!(ir.node(*b).kind, CanonNodeKind::Lifetime { .. }) {
                        self.add_outlives(n.id, *b);
                    }
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

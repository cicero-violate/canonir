use canon::csr_graph::CsrGraph;
use canon::edge::EdgeKind;
use canon::node::{CanonId, CanonNodeKind, CfgOp};
use canon::CanonIR;

pub struct CallGraphBuilder {
    v: usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl CallGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_call(&mut self, caller: CanonId, callee: CanonId) {
        self.edges.push((caller.0, callee.0, EdgeKind::Calls));
    }

    pub fn derive_from_ir(&mut self, ir: &CanonIR) {
        for n in &ir.nodes {
            let CanonNodeKind::Fn { body: Some(body_id), .. } = &n.kind else {
                continue;
            };
            let CanonNodeKind::Body { blocks } = &ir.node(*body_id).kind else {
                continue;
            };
            for bb in blocks {
                let CanonNodeKind::BasicBlock { ops, .. } = &ir.node(*bb).kind else {
                    continue;
                };
                for op in ops {
                    if let CfgOp::Call { func, .. } = op {
                        self.add_call(n.id, *func);
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

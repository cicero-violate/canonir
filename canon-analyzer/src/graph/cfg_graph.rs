use canon::node::{CanonId, CanonNodeKind, CfgOp};
use canon::CanonIR;
use canon::csr_graph::CsrGraph;
use canon::edge::EdgeKind;

pub struct CfgGraphBuilder {
    v: usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl CfgGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_cfg_edge(&mut self, src: CanonId, dst: CanonId) {
        self.edges.push((src.0, dst.0, EdgeKind::CfgEdge));
    }

    pub fn add_branch(&mut self, src: CanonId, dst: CanonId, label: String) {
        self.edges.push((src.0, dst.0, EdgeKind::CfgBranch { label }));
    }

    pub fn derive_from_ir(&mut self, ir: &CanonIR) {
        for n in &ir.nodes {
            let CanonNodeKind::Body { blocks } = &n.kind else {
                continue;
            };

            for (i, bb_id) in blocks.iter().enumerate() {
                let CanonNodeKind::BasicBlock { ops, next } = &ir.node(*bb_id).kind else {
                    continue;
                };

                if let Some(last) = ops.last() {
                    match last {
                        CfgOp::Goto(t) => {
                            if let Some(dst) = blocks.get(*t as usize).copied() {
                                self.add_cfg_edge(*bb_id, dst);
                            }
                        }
                        CfgOp::Branch { true_bb, false_bb, .. } => {
                            if let Some(t) = blocks.get(*true_bb as usize).copied() {
                                self.add_branch(*bb_id, t, "true".into());
                            }
                            if let Some(f) = blocks.get(*false_bb as usize).copied() {
                                self.add_branch(*bb_id, f, "false".into());
                            }
                        }
                        _ => {
                            if let Some(nx) = next.and_then(|ix| blocks.get(ix as usize).copied()) {
                                self.add_cfg_edge(*bb_id, nx);
                            } else if let Some(fallthrough) = blocks.get(i + 1).copied() {
                                self.add_cfg_edge(*bb_id, fallthrough);
                            }
                        }
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

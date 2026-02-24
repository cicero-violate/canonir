//! CFG-graph builder.
//!
//! Variables:
//!   G_cfg : CsrGraph<NodeId, EdgeKind>
//!
//! Edges emitted:
//!   (bb_src, bb_dst, CfgEdge)           — unconditional flow
//!   (bb_src, bb_dst, CfgBranch{label})  — conditional branch with label
//!
//! Each basic block in a Body::Blocks is a node.
//! Terminator::Goto(t)            -> CfgEdge
//! Terminator::Branch{true, false}-> CfgBranch("true"), CfgBranch("false")
//! Terminator::Return             -> no outgoing edge

use model::ir::{
    csr_graph::CsrGraph,
    edge::EdgeKind,
    node::{Body, NodeId, Terminator},
};

pub struct CfgGraphBuilder {
    v:     usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl CfgGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_cfg_edge(&mut self, src: NodeId, dst: NodeId) {
        self.edges.push((src.0, dst.0, EdgeKind::CfgEdge));
    }

    pub fn add_branch(&mut self, src: NodeId, dst: NodeId, label: String) {
        self.edges.push((src.0, dst.0, EdgeKind::CfgBranch { label }));
    }

    /// Derive CFG edges from all Body::Blocks in the node arena.
    /// Basic block indices are local; we offset by `base` NodeId for global ids.
    pub fn add_from_body(&mut self, base: NodeId, body: &Body) {
        if let Body::Blocks(blocks) = body {
            for (i, bb) in blocks.iter().enumerate() {
                let src = NodeId(base.0 + i as u32);
                match &bb.terminator {
                    Terminator::Goto(t) => {
                        let dst = NodeId(base.0 + *t);
                        self.edges.push((src.0, dst.0, EdgeKind::CfgEdge));
                    }
                    Terminator::Branch { true_bb, false_bb, .. } => {
                        let t = NodeId(base.0 + *true_bb);
                        let f = NodeId(base.0 + *false_bb);
                        self.edges.push((src.0, t.0, EdgeKind::CfgBranch { label: "true".into() }));
                        self.edges.push((src.0, f.0, EdgeKind::CfgBranch { label: "false".into() }));
                    }
                    Terminator::Return | Terminator::None => {}
                }
            }
        }
    }

    pub fn build(self) -> CsrGraph<NodeId, EdgeKind> {
        let node_ids: Vec<NodeId> = (0..self.v as u32).map(NodeId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}

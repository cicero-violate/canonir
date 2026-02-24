//! Value-graph builder — const/static dependency edges.
//!
//! Variables:
//!   G_value : CsrGraph<NodeId, EdgeKind>
//!
//! Edges emitted:
//!   (c1, c2, ConstDep) — const item c1 depends on const item c2
//!
//! Populated by: const_solver (future) when IR gains NodeKind::Const/Static.

use model::ir::{csr_graph::CsrGraph, edge::EdgeKind, node::NodeId};

pub struct ValueGraphBuilder {
    v:     usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl ValueGraphBuilder {
    pub fn new(v: usize) -> Self { Self { v, edges: Vec::new() } }

    /// Register: const node `src` depends on const node `dst`.
    /// Equation: ConstDep(src, dst) ∈ G_value
    pub fn add_const_dep(&mut self, src: NodeId, dst: NodeId) {
        self.edges.push((src.0, dst.0, EdgeKind::ConstDep));
    }

    pub fn build(self) -> CsrGraph<NodeId, EdgeKind> {
        let node_ids: Vec<NodeId> = (0..self.v as u32).map(NodeId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}

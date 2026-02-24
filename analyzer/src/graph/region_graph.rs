//! Region-graph builder — lifetime outlives constraints.
//!
//! Variables:
//!   G_region : CsrGraph<NodeId, EdgeKind>
//!
//! Edges emitted:
//!   (a, b, Outlives) — lifetime region a outlives region b
//!
//! Populated by: borrow_solver (future) when IR gains lifetime nodes.

use model::ir::{csr_graph::CsrGraph, edge::EdgeKind, node::NodeId};

pub struct RegionGraphBuilder {
    v:     usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl RegionGraphBuilder {
    pub fn new(v: usize) -> Self { Self { v, edges: Vec::new() } }

    /// Register: lifetime `a` outlives lifetime `b`.
    /// Equation: Outlives(a, b) ∈ G_region
    pub fn add_outlives(&mut self, a: NodeId, b: NodeId) {
        self.edges.push((a.0, b.0, EdgeKind::Outlives));
    }

    pub fn build(self) -> CsrGraph<NodeId, EdgeKind> {
        let node_ids: Vec<NodeId> = (0..self.v as u32).map(NodeId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}

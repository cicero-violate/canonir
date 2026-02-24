//! Macro-graph builder — macro expansion edges.
//!
//! Variables:
//!   G_macro : CsrGraph<NodeId, EdgeKind>
//!
//! Edges emitted:
//!   (macro_node, item_node, Expands) — macro expands to item
//!
//! Populated by: macro_solver (future) when IR gains NodeKind::MacroCall.

use model::ir::{csr_graph::CsrGraph, edge::EdgeKind, node::NodeId};

pub struct MacroGraphBuilder {
    v:     usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl MacroGraphBuilder {
    pub fn new(v: usize) -> Self { Self { v, edges: Vec::new() } }

    /// Register: macro node `src` expands to item node `dst`.
    /// Equation: Expands(src, dst) ∈ G_macro
    pub fn add_expands(&mut self, src: NodeId, dst: NodeId) {
        self.edges.push((src.0, dst.0, EdgeKind::Expands));
    }

    pub fn build(self) -> CsrGraph<NodeId, EdgeKind> {
        let node_ids: Vec<NodeId> = (0..self.v as u32).map(NodeId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}

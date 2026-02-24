//! Name-graph builder.
//!
//! Variables:
//!   G_name : CsrGraph<NodeId, EdgeKind>
//!
//! Edges emitted:
//!   (src, dst, Renames)  — explicit rename constraint: src renames dst
//!   (src, dst, Resolves) — name reference: src resolves to definition dst
//!
//! At capture time, the populator calls `add_rename(src, dst)` or
//! `add_resolves(src, dst)` before handing the edge list to `build()`.

use model::ir::{
    csr_graph::CsrGraph,
    edge::EdgeKind,
    node::NodeId,
};

pub struct NameGraphBuilder {
    v:     usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl NameGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    /// Register a rename constraint: node `src` renames node `dst`.
    pub fn add_rename(&mut self, src: NodeId, dst: NodeId) {
        self.edges.push((src.0, dst.0, EdgeKind::Renames));
    }

    /// Register a name resolution: node `src` resolves to definition `dst`.
    pub fn add_resolves(&mut self, src: NodeId, dst: NodeId) {
        self.edges.push((src.0, dst.0, EdgeKind::Resolves));
    }

    pub fn build(self) -> CsrGraph<NodeId, EdgeKind> {
        let node_ids: Vec<NodeId> = (0..self.v as u32).map(NodeId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}

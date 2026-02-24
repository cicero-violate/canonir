//! Type-graph builder.
//!
//! Variables:
//!   G_type : CsrGraph<NodeId, EdgeKind>
//!
//! Edges emitted:
//!   (src, dst, TypeOf)      — expression src has type dst
//!   (src, dst, TypeUnifies) — src and dst must unify (type inference)

use model::ir::{
    csr_graph::CsrGraph,
    edge::EdgeKind,
    node::NodeId,
};

pub struct TypeGraphBuilder {
    v:     usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl TypeGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_type_of(&mut self, expr: NodeId, ty: NodeId) {
        self.edges.push((expr.0, ty.0, EdgeKind::TypeOf));
    }

    pub fn add_unifies(&mut self, a: NodeId, b: NodeId) {
        self.edges.push((a.0, b.0, EdgeKind::TypeUnifies));
    }

    pub fn build(self) -> CsrGraph<NodeId, EdgeKind> {
        let node_ids: Vec<NodeId> = (0..self.v as u32).map(NodeId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}

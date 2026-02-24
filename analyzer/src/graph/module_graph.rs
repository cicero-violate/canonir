//! Module-graph builder.
//!
//! Variables:
//!   G_module : CsrGraph<NodeId, EdgeKind>
//!
//! Edges emitted:
//!   (module, item, Contains) — module contains item
//!   (impl,   item, Contains) — impl block contains method
//!   (impl,   struct, ImplFor) — impl targets struct

use model::ir::{
    csr_graph::CsrGraph,
    edge::EdgeKind,
    node::NodeId,
};

pub struct ModuleGraphBuilder {
    v:     usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl ModuleGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_contains(&mut self, parent: NodeId, child: NodeId) {
        self.edges.push((parent.0, child.0, EdgeKind::Contains));
    }

    pub fn add_impl_for(&mut self, impl_node: NodeId, struct_node: NodeId) {
        self.edges.push((impl_node.0, struct_node.0, EdgeKind::ImplFor));
    }

    pub fn build(self) -> CsrGraph<NodeId, EdgeKind> {
        let node_ids: Vec<NodeId> = (0..self.v as u32).map(NodeId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}

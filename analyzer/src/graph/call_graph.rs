//! Call-graph builder.
//!
//! Variables:
//!   G_call : CsrGraph<NodeId, EdgeKind>
//!
//! Edges emitted:
//!   (src, dst, Calls) — function/method src calls function/method dst

use model::ir::{
    csr_graph::CsrGraph,
    edge::EdgeKind,
    node::{NodeId, NodeKind},
};

pub struct CallGraphBuilder {
    v:     usize,
    edges: Vec<(u32, u32, EdgeKind)>,
}

impl CallGraphBuilder {
    pub fn new(v: usize) -> Self {
        Self { v, edges: Vec::new() }
    }

    pub fn add_call(&mut self, caller: NodeId, callee: NodeId) {
        self.edges.push((caller.0, callee.0, EdgeKind::Calls));
    }

    /// Auto-derive call edges from Effects::calls strings by matching names.
    /// nodes: full node arena for name lookup.
    pub fn add_calls_from_effects(
        &mut self,
        caller: NodeId,
        call_names: &[String],
        nodes: &[model::ir::node::Node],
    ) {
        for name in call_names {
            for node in nodes {
                let matches = match &node.kind {
                    NodeKind::Function { name: n, .. } => n == name,
                    NodeKind::Method   { name: n, .. } => n == name,
                    _ => false,
                };
                if matches {
                    self.edges.push((caller.0, node.id.0, EdgeKind::Calls));
                }
            }
        }
    }

    pub fn build(self) -> CsrGraph<NodeId, EdgeKind> {
        let node_ids: Vec<NodeId> = (0..self.v as u32).map(NodeId).collect();
        CsrGraph::from_edges(node_ids, self.edges)
    }
}

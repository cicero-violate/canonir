//! ModelIR — the central intermediate representation.
//!
//! Variables:
//!   nodes        : Vec<Node>                      — flat node arena
//!   name_graph   : CsrGraph<NodeId, EdgeKind>     — G_name
//!   type_graph   : CsrGraph<NodeId, EdgeKind>     — G_type
//!   call_graph   : CsrGraph<NodeId, EdgeKind>     — G_call
//!   module_graph : CsrGraph<NodeId, EdgeKind>     — G_module
//!   cfg_graph    : CsrGraph<NodeId, EdgeKind>     — G_cfg
//!   region_graph : CsrGraph<NodeId, EdgeKind>     — G_region
//!   value_graph  : CsrGraph<NodeId, EdgeKind>     — G_value
//!   macro_graph  : CsrGraph<NodeId, EdgeKind>     — G_macro
//!
//! Pipeline:
//!   capture  ->  ModelIR  ->  derive()  ->  solve()  ->  emit()

use crate::ir::{
    csr_graph::CsrGraph,
    edge::EdgeKind,
    node::{Node, NodeId, NodeKind},
};
use crate::ir::edge::EdgeHint;
use serde::{Deserialize, Serialize};

/// The full intermediate representation of a Rust workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelIR {
    pub version: String,
    /// Flat arena — all NodeIds index into this.
    pub nodes: Vec<Node>,
    /// Topological emit order produced by module_solver.
    pub emit_order: Vec<NodeId>,
    /// Explicit edge hints provided by JSON author or capture layer.
    /// derive() distributes these into the five CSR graphs.
    pub edge_hints: Vec<EdgeHint>,
    /// G_name  — rename / name-resolution constraints.
    pub name_graph: CsrGraph<NodeId, EdgeKind>,
    /// G_type  — type inference / unification edges.
    pub type_graph: CsrGraph<NodeId, EdgeKind>,
    /// G_call  — caller → callee edges.
    pub call_graph: CsrGraph<NodeId, EdgeKind>,
    /// G_module — containment: module → item.
    pub module_graph: CsrGraph<NodeId, EdgeKind>,
    /// G_cfg   — control-flow edges within function bodies.
    pub cfg_graph: CsrGraph<NodeId, EdgeKind>,
    /// G_region — lifetime outlives constraints (borrow solver).
    #[serde(default)]
    pub region_graph: CsrGraph<NodeId, EdgeKind>,
    /// G_value  — const/static dependency edges (const solver).
    #[serde(default)]
    pub value_graph: CsrGraph<NodeId, EdgeKind>,
    /// G_macro  — macro expansion edges (macro solver).
    #[serde(default)]
    pub macro_graph: CsrGraph<NodeId, EdgeKind>,
}

impl ModelIR {
    pub fn new() -> Self {
        Self {
            version: "0.2".into(),
            nodes: Vec::new(),
            emit_order: Vec::new(),
            edge_hints: Vec::new(),
            name_graph: CsrGraph::empty(),
            type_graph: CsrGraph::empty(),
            call_graph: CsrGraph::empty(),
            module_graph: CsrGraph::empty(),
            cfg_graph: CsrGraph::empty(),
            region_graph: CsrGraph::empty(),
            value_graph: CsrGraph::empty(),
            macro_graph: CsrGraph::empty(),
        }
    }

    /// Allocate a new node and return its NodeId.
    pub fn push_node(&mut self, kind: NodeKind, span: Option<String>) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node { id, kind, span });
        id
    }

    /// Convenience: look up a node by id.
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }
}

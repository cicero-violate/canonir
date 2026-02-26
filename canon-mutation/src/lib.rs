pub mod apply;
pub mod diff;
pub mod verify;

use canon::node::{CanonId, CanonNodeKind};
use canon::edge::EdgeKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MutationOp {
    AddNode { kind: CanonNodeKind },
    RemoveNode { id: CanonId },
    UpdateNode { id: CanonId, kind: CanonNodeKind },
    AddEdge { src: CanonId, dst: CanonId, kind: EdgeKind },
    RemoveEdge { src: CanonId, dst: CanonId, kind: EdgeKind },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub src: CanonId,
    pub dst: CanonId,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSet {
    pub added_nodes: Vec<CanonId>,
    pub removed_nodes: Vec<CanonId>,
    pub changed_nodes: Vec<(CanonId, CanonNodeKind, CanonNodeKind)>,
    pub added_edges: Vec<GraphEdge>,
    pub removed_edges: Vec<GraphEdge>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty() && self.removed_nodes.is_empty() && self.changed_nodes.is_empty() && self.added_edges.is_empty() && self.removed_edges.is_empty()
    }
}

pub use apply::apply;
pub use diff::diff;
pub use verify::verify;

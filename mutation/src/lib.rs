//! Mutation pipeline — atomic IR mutations, diffing, and verification.
//!
//! Variables:
//!   IR       : ModelIR          — the mutable intermediate representation
//!   op       : MutationOp       — one atomic change to apply
//!   snap_A   : ModelIR (clone)  — baseline snapshot before mutation
//!   snap_B   : ModelIR (clone)  — snapshot after mutation
//!   Δ        : ChangeSet        — diff(snap_A, snap_B)
//!
//! Pipeline:
//!   load → snapshot_A → apply(op) → diff(A, current) → verify → emit → snapshot_B
//!
//! Equations:
//!   snap_A   = clone(IR)
//!   IR'      = apply(IR, op)
//!   Δ        = diff(snap_A, IR')
//!   valid    = verify(IR')    — re-runs analyze() + invariant_solver

pub mod apply;
pub mod diff;
pub mod verify;

use model::ir::{
    edge::EdgeHint,
    node::{NodeId, NodeKind},
};
use serde::{Deserialize, Serialize};

// ── MutationOp ───────────────────────────────────────────────────────────────

/// One atomic mutation on ModelIR.
///
/// Equations:
///   AddNode(kind)       : IR' = IR ∪ { Node(|V|, kind) }
///   RemoveNode(id)      : IR' = IR \ { id }  (edges incident to id also removed)
///   UpdateNode(id,kind) : IR'[id].kind = kind
///   AddEdge(hint)       : IR'.edge_hints = IR.edge_hints ∪ { hint }
///   RemoveEdge(src,dst,k): IR'.edge_hints = IR.edge_hints \ { (src,dst,k) }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MutationOp {
    AddNode    { kind: NodeKind, span: Option<String> },
    RemoveNode { id: NodeId },
    UpdateNode { id: NodeId, kind: NodeKind },
    AddEdge    { hint: EdgeHint },
    RemoveEdge { src: NodeId, dst: NodeId, kind: model::ir::edge::EdgeKind },
}

// ── ChangeSet ────────────────────────────────────────────────────────────────

/// Result of diffing two ModelIR snapshots.
///
/// Equations:
///   added_nodes   = V(IR') \ V(IR)
///   removed_nodes = V(IR)  \ V(IR')
///   changed_nodes = { (id, before, after) | IR[id] ≠ IR'[id] }
///   added_edges   = E(IR') \ E(IR)
///   removed_edges = E(IR)  \ E(IR')
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSet {
    pub added_nodes:   Vec<NodeId>,
    pub removed_nodes: Vec<NodeId>,
    pub changed_nodes: Vec<(NodeId, NodeKind, NodeKind)>, // (id, before, after)
    pub added_edges:   Vec<EdgeHint>,
    pub removed_edges: Vec<EdgeHint>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.changed_nodes.is_empty()
            && self.added_edges.is_empty()
            && self.removed_edges.is_empty()
    }
}

// ── top-level convenience re-exports ─────────────────────────────────────────

pub use apply::apply;
pub use diff::diff;
pub use verify::verify;

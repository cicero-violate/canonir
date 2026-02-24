//! apply — execute one MutationOp against a ModelIR in place.
//!
//! Variables:
//!   ir  : &mut ModelIR   — mutated in place
//!   op  : MutationOp     — the operation to execute
//!   id  : NodeId         — returned: the affected node id
//!
//! Equations:
//!   AddNode    : id = |ir.nodes|;  ir.nodes[id] = Node { id, kind, span }
//!   RemoveNode : ir.nodes[id].kind = NodeKind::Tombstone (logical delete)
//!                ir.edge_hints.retain(|h| h.src≠id ∧ h.dst≠id)
//!   UpdateNode : ir.nodes[id].kind = kind
//!   AddEdge    : ir.edge_hints.push(hint)
//!   RemoveEdge : ir.edge_hints.retain(|h| ¬matches(h, src,dst,kind))

use anyhow::{bail, Result};
use model::ir::{
    model_ir::ModelIR,
    node::{Node, NodeId, NodeKind},
};
use crate::MutationOp;

/// Apply one mutation to `ir` and return the primary affected NodeId.
pub fn apply(ir: &mut ModelIR, op: MutationOp) -> Result<NodeId> {
    match op {
        // ── AddNode ──────────────────────────────────────────────────────────
        // Equation: id = |V|;  V' = V ∪ { Node(id, kind, span) }
        MutationOp::AddNode { kind, span } => {
            let id = NodeId(ir.nodes.len() as u32);
            ir.nodes.push(Node { id, kind, span });
            Ok(id)
        }

        // ── RemoveNode ───────────────────────────────────────────────────────
        // Equation: V' = V \ { id };  E' = E \ { e | e.src=id ∨ e.dst=id }
        // We use a Tombstone rather than compacting indices to preserve NodeId stability.
        MutationOp::RemoveNode { id } => {
            let idx = id.index();
            if idx >= ir.nodes.len() {
                bail!("apply::RemoveNode: NodeId {} out of range (|V|={})", idx, ir.nodes.len());
            }
            // Replace kind with tombstone — emitter skips Unknown nodes.
            ir.nodes[idx].kind = NodeKind::TypeRef { name: "__tombstone__".into() };
            // Remove all edge_hints incident to this node.
            // Equation: E' = { h ∈ E | h.src ≠ id.0 ∧ h.dst ≠ id.0 }
            ir.edge_hints.retain(|h| h.src != id.0 && h.dst != id.0);
            Ok(id)
        }

        // ── UpdateNode ───────────────────────────────────────────────────────
        // Equation: ir.nodes[id].kind = kind
        MutationOp::UpdateNode { id, kind } => {
            let idx = id.index();
            if idx >= ir.nodes.len() {
                bail!("apply::UpdateNode: NodeId {} out of range (|V|={})", idx, ir.nodes.len());
            }
            ir.nodes[idx].kind = kind;
            Ok(id)
        }

        // ── AddEdge ──────────────────────────────────────────────────────────
        // Equation: E' = E ∪ { hint }
        MutationOp::AddEdge { hint } => {
            let src = NodeId(hint.src);
            let dst = NodeId(hint.dst);
            if src.index() >= ir.nodes.len() {
                bail!("apply::AddEdge: src {} out of range", hint.src);
            }
            if dst.index() >= ir.nodes.len() {
                bail!("apply::AddEdge: dst {} out of range", hint.dst);
            }
            ir.edge_hints.push(hint.clone());
            Ok(src)
        }

        // ── RemoveEdge ───────────────────────────────────────────────────────
        // Equation: E' = E \ { h | h.src=src ∧ h.dst=dst ∧ h.kind=kind }
        MutationOp::RemoveEdge { src, dst, kind } => {
            ir.edge_hints.retain(|h| {
                !(h.src == src.0 && h.dst == dst.0 && h.kind == kind)
            });
            Ok(src)
        }
    }
}

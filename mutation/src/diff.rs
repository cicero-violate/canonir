//! diff — compute ChangeSet between two ModelIR snapshots.
//!
//! Variables:
//!   before, after : &ModelIR   — two snapshots to compare
//!   Δ             : ChangeSet  — result
//!
//! Equations:
//!   added_nodes   = { id | id < |after.nodes| ∧ id ≥ |before.nodes| }
//!   removed_nodes = { id | before.nodes[id].kind = Tombstone }
//!   changed_nodes = { (id, bk, ak) | id < min(|B|,|A|) ∧ bk ≠ ak ∧ bk ≠ Tombstone }
//!   added_edges   = after.edge_hints  \ before.edge_hints
//!   removed_edges = before.edge_hints \ after.edge_hints

use model::ir::{edge::EdgeHint, model_ir::ModelIR, node::{NodeId, NodeKind}};
use crate::ChangeSet;
use std::collections::HashSet;

pub fn diff(before: &ModelIR, after: &ModelIR) -> ChangeSet {
    // ── nodes ─────────────────────────────────────────────────────────────
    let bn = before.nodes.len();
    let an = after.nodes.len();

    // added: indices that did not exist in before
    // Equation: added_nodes = { NodeId(i) | i ∈ [bn, an) }
    let added_nodes: Vec<NodeId> = (bn..an).map(|i| NodeId(i as u32)).collect();

    // changed + removed: walk shared index range
    // Equation:
    //   tombstone(n) <=> n.kind = TypeRef { name: "__tombstone__" }
    //   removed(id)  <=> tombstone(after[id]) ∧ ¬tombstone(before[id])
    //   changed(id)  <=> before[id].kind ≠ after[id].kind ∧ ¬tombstone(after[id])
    let mut removed_nodes = Vec::new();
    let mut changed_nodes = Vec::new();

    let is_tombstone = |k: &NodeKind| matches!(k, NodeKind::TypeRef { name } if name == "__tombstone__");

    for i in 0..bn.min(an) {
        let bk = &before.nodes[i].kind;
        let ak = &after.nodes[i].kind;
        if bk == ak { continue; }
        if is_tombstone(ak) && !is_tombstone(bk) {
            removed_nodes.push(NodeId(i as u32));
        } else if !is_tombstone(ak) {
            changed_nodes.push((NodeId(i as u32), bk.clone(), ak.clone()));
        }
    }

    // ── edges ─────────────────────────────────────────────────────────────
    // Equation:
    //   added_edges   = after.edge_hints  \ before.edge_hints
    //   removed_edges = before.edge_hints \ after.edge_hints
    let before_edges: HashSet<EdgeHintKey> = before.edge_hints.iter().map(EdgeHintKey::from).collect();
    let after_edges:  HashSet<EdgeHintKey> = after.edge_hints.iter().map(EdgeHintKey::from).collect();

    let added_edges: Vec<EdgeHint> = after.edge_hints.iter()
        .filter(|h| !before_edges.contains(&EdgeHintKey::from(*h)))
        .cloned().collect();
    let removed_edges: Vec<EdgeHint> = before.edge_hints.iter()
        .filter(|h| !after_edges.contains(&EdgeHintKey::from(*h)))
        .cloned().collect();

    ChangeSet { added_nodes, removed_nodes, changed_nodes, added_edges, removed_edges }
}

// EdgeHint doesn't derive Hash/Eq (EdgeKind has CfgBranch with a String field),
// so we use a JSON-string key for set membership.
#[derive(PartialEq, Eq, Hash)]
struct EdgeHintKey(String);

impl From<&EdgeHint> for EdgeHintKey {
    fn from(h: &EdgeHint) -> Self {
        EdgeHintKey(format!("{}-{}-{:?}", h.src, h.dst, h.kind))
    }
}

//! Name-graph solver — rename constraint propagation.
//!
//! Variables:
//!   G_name  = ir.name_graph          (CSR)
//!   order   = topological_sort(adj)  — processing order
//!   rename  : NodeId -> Option<String>  — new name per node
//!
//! Equations:
//!   order = topo(G_name)
//!   for v in order:
//!     for (dst, EdgeKind::Renames) in G_name.neighbours(v):
//!       node[dst].name <- node[v].name   (propagate rename)
//!
//! Algorithm used: topological_sort (Kahn's, from algorithms crate)

use anyhow::Result;
use model::ir::{edge::EdgeKind, model_ir::ModelIR, node::NodeKind};
use algorithms::graph::topological_sort::topological_sort;
use crate::solver::csr_to_adj;

pub fn solve(ir: &mut ModelIR) -> Result<()> {
    let v = ir.name_graph.vertex_count();
    if v == 0 { return Ok(()); }

    let adj   = csr_to_adj(&ir.name_graph);
    let order = topological_sort(&adj);

    // Collect (dst_index, new_name) pairs first to avoid borrow conflict.
    let mut renames: Vec<(usize, String)> = Vec::new();

    for src_idx in &order {
        let src_id = model::ir::node::NodeId(*src_idx as u32);
        // Extract current name of src node.
        let src_name = node_name(&ir.nodes[*src_idx].kind).map(|s| s.to_owned());

        if let Some(name) = src_name {
            for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
                if *edge == EdgeKind::Renames {
                    renames.push((dst_id.index(), name.clone()));
                }
            }
        }
    }

    // Apply renames.
    for (idx, new_name) in renames {
        apply_rename(&mut ir.nodes[idx].kind, new_name);
    }

    Ok(())
}

fn node_name(kind: &NodeKind) -> Option<&str> {
    match kind {
        NodeKind::Struct   { name, .. } => Some(name),
        NodeKind::Trait    { name, .. } => Some(name),
        NodeKind::Function { name, .. } => Some(name),
        NodeKind::Method   { name, .. } => Some(name),
        NodeKind::TypeRef  { name }     => Some(name),
        NodeKind::Impl { for_struct, .. } => Some(for_struct),
        _ => None,
    }
}

fn apply_rename(kind: &mut NodeKind, new_name: String) {
    match kind {
        NodeKind::Struct   { name, .. } => *name = new_name,
        NodeKind::Trait    { name, .. } => *name = new_name,
        NodeKind::Function { name, .. } => *name = new_name,
        NodeKind::Method   { name, .. } => *name = new_name,
        NodeKind::TypeRef  { name }     => *name = new_name,
        NodeKind::Impl { for_struct, .. } => *for_struct = new_name,
        _ => {}
    }
}

//! Unsafe Soundness Solver (S15).
//!
//! Variables:
//!   U = { v | v.unsafe_ = true }   — unsafe fns, impls, traits
//!   G_call = ir.call_graph          — caller -> callee edges
//!
//! Equation:
//!   sound(v) <=> v ∈ U  =>  ∀ (u ->calls-> v): u ∈ U
//!   i.e. an unsafe fn may only be called from another unsafe fn/block.
//!
//! Current implementation: warns on safe callers of unsafe callees.
//! (Block-level unsafe context not yet tracked in IR — gap E12b.)

use anyhow::Result;
use model::ir::{model_ir::ModelIR, node::NodeKind};

pub fn solve(ir: &ModelIR) -> Result<()> {
    if ir.call_graph.vertex_count() == 0 {
        return Ok(());
    }

    // Build set of unsafe node indices.
    // Equation: U = { i | nodes[i].unsafe_ }
    let unsafe_set: Vec<bool> = ir.nodes.iter()
        .map(|n| match &n.kind {
            NodeKind::Function { unsafe_, .. } => *unsafe_,
            NodeKind::Method   { unsafe_, .. } => *unsafe_,
            NodeKind::Impl     { unsafe_, .. } => *unsafe_,
            NodeKind::Trait    { unsafe_, .. } => *unsafe_,
            _ => false,
        })
        .collect();

    let v = ir.call_graph.vertex_count().min(ir.nodes.len());

    for caller_idx in 0..v {
        let caller_id = model::ir::node::NodeId(caller_idx as u32);
        let caller_unsafe = unsafe_set.get(caller_idx).copied().unwrap_or(false);
        for (callee_id, _) in ir.call_graph.neighbours(caller_id) {
            let callee_unsafe = unsafe_set.get(callee_id.index()).copied().unwrap_or(false);
            // Equation: callee ∈ U ∧ caller ∉ U  =>  WARN
            if callee_unsafe && !caller_unsafe {
                let caller_name = node_name(ir, caller_idx);
                let callee_name = node_name(ir, callee_id.index());
                log::warn!(
                    "unsafe_solver: safe fn `{}` calls unsafe fn `{}` without unsafe block",
                    caller_name, callee_name
                );
            }
        }
    }

    Ok(())
}

fn node_name(ir: &ModelIR, idx: usize) -> String {
    ir.nodes.get(idx).map(|n| match &n.kind {
        NodeKind::Function { name, .. } => name.clone(),
        NodeKind::Method   { name, .. } => name.clone(),
        _ => format!("node_{}", idx),
    }).unwrap_or_else(|| format!("node_{}", idx))
}

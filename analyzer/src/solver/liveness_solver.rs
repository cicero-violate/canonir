//! Call Graph Liveness Solver (S8) — remove dead functions from emit_order.
//!
//! Variables:
//!   G_call  = ir.call_graph     (Calls edges)
//!   roots   = { v | NodeKind::Function { name: "main" } }
//!             ∪ { v | NodeKind::Crate  }   (all pub fns are live at crate root)
//!   live    = reachability(G_call, roots)
//!   dead(v) <=> v ∈ Function ∧ ¬live(v)
//!
//! Equation:
//!   emit_order' = filter(emit_order, ¬dead)

use anyhow::Result;
use model::ir::{model_ir::ModelIR, node::{NodeKind, Visibility}};
use algorithms::graph::reachability::reachability;
use crate::solver::csr_to_adj;

pub fn solve(ir: &mut ModelIR) -> Result<()> {
    let call_v = ir.call_graph.vertex_count();
    if call_v == 0 { return Ok(()); }

    let adj = csr_to_adj(&ir.call_graph);

    // Roots: main functions + all pub functions (conservatively live).
    // Equation: roots = { v | fn_name(v)="main" } ∪ { v | vis(v)=Public }
    let roots: Vec<usize> = ir.nodes.iter().enumerate().filter_map(|(idx, n)| {
        match &n.kind {
            NodeKind::Function { name, vis, .. } => {
                if name == "main" || *vis == Visibility::Public { Some(idx) } else { None }
            }
            NodeKind::Crate { .. } => Some(idx),
            _ => None,
        }
    }).filter(|&idx| idx < call_v).collect();

    if roots.is_empty() { return Ok(()); }

    let live = reachability(&adj, &roots);

    // Remove dead Function nodes from emit_order.
    // Equation: emit_order' = [ v | v ∈ emit_order ∧ (¬Function(v) ∨ live[v]) ]
    let before = ir.emit_order.len();
    ir.emit_order.retain(|&id| {
        let idx = id.index();
        match ir.nodes.get(idx).map(|n| &n.kind) {
            Some(NodeKind::Function { vis, .. }) => {
                if idx < live.len() && live[idx] { return true; }
                if *vis == Visibility::Public     { return true; }
                false
            }
            _ => true,
        }
    });
    let removed = before - ir.emit_order.len();
    if removed > 0 {
        eprintln!("INFO liveness_solver: pruned {} dead function(s) from emit_order", removed);
    }

    Ok(())
}

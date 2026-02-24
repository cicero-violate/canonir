//! Type Cycle Diagnostic Solver (S7) — structured error output for SCC cycles.
//!
//! Variables:
//!   sccs      = kosaraju_scc(G_type)
//!   cycle_scc = { scc ∈ sccs | |scc| > 1 }
//!
//! Equations:
//!   cycle_edges(scc) = { (u,v) | (u,v,TypeUnifies) ∈ G_type ∧ u∈scc ∧ v∈scc }
//!   diag(scc) = "type cycle: " ++ join(", ", map(name, scc))

use anyhow::Result;
use model::ir::{model_ir::ModelIR, node::NodeKind};
use algorithms::graph::scc::kosaraju_scc;
use crate::solver::csr_to_adj;

fn node_label(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Struct    { name, .. } => format!("struct {}", name),
        NodeKind::Trait     { name, .. } => format!("trait {}", name),
        NodeKind::Function  { name, .. } => format!("fn {}", name),
        NodeKind::Method    { name, .. } => format!("method {}", name),
        NodeKind::TypeAlias { name, .. } => format!("type {}", name),
        NodeKind::TypeRef   { name }     => format!("ref {}", name),
        _ => "?".to_string(),
    }
}

pub fn solve(ir: &ModelIR) -> Result<()> {
    let v = ir.type_graph.vertex_count();
    if v == 0 { return Ok(()); }

    let adj  = csr_to_adj(&ir.type_graph);
    let sccs = kosaraju_scc(&adj);

    // Emit a structured diagnostic for each non-trivial SCC.
    // Equation: diag(scc) = "type cycle: " ++ join(", ", labels)
    for scc in sccs.iter().filter(|s| s.len() > 1) {
        let labels: Vec<String> = scc.iter().filter_map(|&idx| {
            ir.nodes.get(idx).map(|n| node_label(&n.kind))
        }).collect();
        eprintln!(
            "DIAG cycle_diag_solver: type cycle detected [{}]: {}",
            scc.len(),
            labels.join(" -> ")
        );
    }

    Ok(())
}

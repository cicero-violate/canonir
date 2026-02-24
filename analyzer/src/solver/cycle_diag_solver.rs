//! Type Cycle Diagnostic Solver (S7) — structured error output for SCC cycles.
//!
//! Variables:
//!   sccs      = kosaraju_scc(G_type)
//!   cycle_scc = { scc ∈ sccs | |scc| > 1 }
//!
//! Equations:
//!   cycle_edges(scc) = { (u,v) | (u,v,TypeUnifies) ∈ G_type ∧ u∈scc ∧ v∈scc }
//!   diag_label(scc)  = "cycle:" ++ join(" -> ", map(node_label, scc))
//!   diag_node(scc)   = NodeKind::TypeRef { name: diag_label(scc) }
//!   inject(scc)      = push diag_node into ir.nodes + push diag_node.id into ir.emit_order

use anyhow::Result;
use model::ir::{model_ir::ModelIR, node::{Node, NodeId, NodeKind}};
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

pub fn solve(ir: &mut ModelIR) -> Result<()> {
    let v = ir.type_graph.vertex_count();
    if v == 0 { return Ok(()); }

    let adj  = csr_to_adj(&ir.type_graph);
    let sccs = kosaraju_scc(&adj);

    // For each non-trivial SCC: log diagnostic AND inject a TypeRef node.
    // Equation:
    //   diag_label = "cycle:" ++ labels.join(" -> ")
    //   diag_node  = NodeKind::TypeRef { name: diag_label }
    for scc in sccs.iter().filter(|s| s.len() > 1) {
        let labels: Vec<String> = scc.iter().filter_map(|&idx| {
            ir.nodes.get(idx).map(|n| node_label(&n.kind))
        }).collect();
        let diag_label = format!("cycle:{}", labels.join(" -> "));
        log::debug!(
            "DIAG cycle_diag_solver: type cycle detected [{}]: {}",
            scc.len(),
            &diag_label[6..] // strip "cycle:" prefix for display
        );
        // Inject a TypeRef node so the cycle is visible in the solved IR / emit_order.
        let diag_id = NodeId(ir.nodes.len() as u32);
        ir.nodes.push(Node {
            id: diag_id,
            kind: NodeKind::TypeRef { name: diag_label },
            span: None,
        });
        ir.emit_order.push(diag_id);
    }

    Ok(())
}

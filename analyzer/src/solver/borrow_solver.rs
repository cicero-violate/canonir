//! Borrow & Lifetime Solver (S9) — ACTIVE.
//!
//! Variables:
//!   G_region : CsrGraph  — Outlives edges between Lifetime nodes
//!   adj      : Vec<Vec<usize>>  — adjacency list of G_region
//!   cycles   : Vec<Vec<usize>>  — SCCs of size > 1 in adj
//!
//! Equations:
//!   adj[u]   = { v | (u, v, Outlives) ∈ G_region }
//!   cycles   = outlives_cycles(adj)
//!   valid    <=> cycles = ∅
//!   conflict(a,b) <=> a ∈ scc ∧ b ∈ scc ∧ |scc| > 1
//! Algorithm: algorithms::graph::region::outlives_cycles (Kosaraju SCC filter).

use algorithms::graph::region::outlives_cycles;
use anyhow::bail;
use anyhow::Result;
use model::ir::{model_ir::ModelIR, node::NodeKind};

pub fn solve(ir: &ModelIR) -> Result<()> {
    let v = ir.region_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    // Build adjacency list from G_region.
    // Equation: adj[u] = neighbours of u in G_region
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); v];
    for (idx, node) in ir.nodes.iter().enumerate() {
        if idx >= v {
            break;
        }
        if !matches!(&node.kind, NodeKind::Lifetime { .. }) {
            continue;
        }
        for (nb, _edge) in ir.region_graph.neighbours(node.id) {
            adj[idx].push(nb.index());
        }
    }

    // Equation: cycles = outlives_cycles(adj)
    let cycles = outlives_cycles(&adj);
    if cycles.is_empty() {
        log::info!(
            "borrow_solver: region graph is acyclic — {} lifetime node(s) valid",
            v
        );
        return Ok(());
    }

    // Collect names of conflicting lifetimes for the error message.
    let names: Vec<String> = cycles
        .iter()
        .map(|scc| {
            scc.iter()
                .filter_map(|&i| {
                    ir.nodes.get(i).and_then(|n| {
                        if let NodeKind::Lifetime { name } = &n.kind {
                            Some(format!("'{}", name))
                        } else {
                            None
                        }
                    })
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect();

    bail!(
        "borrow_solver: conflicting lifetime constraints — cycles: [{}]",
        names.join("; ")
    );
}

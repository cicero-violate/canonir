//! Mutation Invariant Solver (S1) — structural safety checks.
//!
//! Variables:
//!   V   = |ir.nodes|
//!   E_k = edges in graph k (k ∈ {name, type, call, module, cfg})
//!
//! Equations:
//!   valid_edge(s, d) <=> s < V ∧ d < V
//!   valid_impl(i)    <=> ∃ v ∈ V: NodeKind::Struct { name } = ir.nodes[v].kind
//!                                 ∧ name = ir.nodes[i].kind.for_struct
//!   acyclic_module   <=> is_acyclic(G_module)
//!
//!   invariant(IR) <=> (∀ e ∈ ∪E_k: valid_edge(e))
//!                    ∧ (∀ i: Impl: valid_impl(i))
//!                    ∧ acyclic_module

use anyhow::{bail, Result};
use model::ir::{model_ir::ModelIR, node::NodeKind};
use algorithms::graph::reachability::is_acyclic;
use crate::solver::csr_to_adj;

pub fn solve(ir: &ModelIR) -> Result<()> {
    let v = ir.nodes.len();

    // ── 1. All edges reference valid node indices ────────────────────────────
    // Equation: ∀ (src, dst) ∈ E_k : src < V ∧ dst < V
    let graphs: &[&dyn Fn() -> Vec<Vec<usize>>] = &[
        &|| csr_to_adj(&ir.name_graph),
        &|| csr_to_adj(&ir.type_graph),
        &|| csr_to_adj(&ir.call_graph),
        &|| csr_to_adj(&ir.module_graph),
        &|| csr_to_adj(&ir.cfg_graph),
    ];
    let graph_names = ["name", "type", "call", "module", "cfg"];
    for (name, g) in graph_names.iter().zip(graphs.iter()) {
        let adj = g();
        for (src, neighbours) in adj.iter().enumerate() {
            for &dst in neighbours {
                if src >= v || dst >= v {
                    bail!(
                        "invariant_solver: dangling edge in {}_graph: {} -> {} (|V|={})",
                        name, src, dst, v
                    );
                }
            }
        }
    }

    // ── 2. Every Impl.for_struct names a Struct that exists ─────────────────
    // Equation: valid_impl(i) <=> ∃ j: Struct { name } where name == for_struct
    let struct_names: std::collections::HashSet<&str> = ir.nodes.iter().filter_map(|n| {
        if let NodeKind::Struct { name, .. } = &n.kind { Some(name.as_str()) } else { None }
    }).collect();

    for (idx, node) in ir.nodes.iter().enumerate() {
        if let NodeKind::Impl { for_struct, .. } = &node.kind {
            if !struct_names.contains(for_struct.as_str()) {
                bail!(
                    "invariant_solver: Impl node {} references unknown struct {:?}",
                    idx, for_struct
                );
            }
        }
    }

    // ── 3. Module graph must be acyclic ─────────────────────────────────────
    // Equation: acyclic(G_module) — module containment cannot be cyclic
    let mod_v = ir.module_graph.vertex_count();
    if mod_v > 0 {
        let adj = csr_to_adj(&ir.module_graph);
        if !is_acyclic(&adj) {
            bail!("invariant_solver: module_graph contains a cycle");
        }
    }

    Ok(())
}

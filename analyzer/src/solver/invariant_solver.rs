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

use crate::solver::csr_to_adj;
use algorithms::graph::reachability::is_acyclic;
use anyhow::{bail, Result};
use model::ir::edge::EdgeKind;
use model::ir::{model_ir::ModelIR, node::NodeKind};

pub fn solve(ir: &ModelIR) -> Result<()> {
    let v = ir.nodes.len();

    // ── 1. All edges reference valid node indices ────────────────────────────
    // Equation: ∀ (src, dst) ∈ E_k : src < V ∧ dst < V
    let graphs: &[&dyn Fn() -> Vec<Vec<usize>>] =
        &[&|| csr_to_adj(&ir.name_graph), &|| csr_to_adj(&ir.type_graph), &|| csr_to_adj(&ir.call_graph), &|| csr_to_adj(&ir.module_graph), &|| csr_to_adj(&ir.cfg_graph)];
    let graph_names = ["name", "type", "call", "module", "cfg"];
    for (name, g) in graph_names.iter().zip(graphs.iter()) {
        let adj = g();
        for (src, neighbours) in adj.iter().enumerate() {
            for &dst in neighbours {
                if src >= v || dst >= v {
                    bail!("invariant_solver: dangling edge in {}_graph: {} -> {} (|V|={})", name, src, dst, v);
                }
            }
        }
    }

    // ── 2. Renames edges must connect same-kinded name-bearing nodes ─────────
    // Equation: valid_rename(s, d) <=> node_kind_tag(s) == node_kind_tag(d)
    //   A Function renaming a Struct is always a data error.
    let name_v = ir.name_graph.vertex_count();
    for src_idx in 0..name_v {
        let src_id = model::ir::node::NodeId(src_idx as u32);
        for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
            if *edge != model::ir::edge::EdgeKind::Renames {
                continue;
            }
            let dst_idx = dst_id.index();
            if src_idx >= v || dst_idx >= v {
                continue;
            } // caught by check 1
            let src_tag = node_kind_tag(&ir.nodes[src_idx].kind);
            let dst_tag = node_kind_tag(&ir.nodes[dst_idx].kind);
            if src_tag != dst_tag {
                bail!("invariant_solver: illegal Renames edge {} ({}) -> {} ({}): kind mismatch", src_idx, src_tag, dst_idx, dst_tag);
            }
        }
    }

    // ── 3. Every Impl.for_struct names a Struct that exists ─────────────────
    // Equation: valid_impl(i) <=> ∃ j: Struct { name } where name == for_struct
    // Equation: valid_impl_target(name) <=> ∃ node with matching name that is
    //   Struct | Enum | Trait | TypeAlias  (all legal impl targets in Rust)
    let impl_target_names: std::collections::HashSet<&str> = ir
        .nodes
        .iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Struct { name, .. } => Some(name.as_str()),
            NodeKind::Enum { name, .. } => Some(name.as_str()),
            NodeKind::Trait { name, .. } => Some(name.as_str()),
            NodeKind::TypeAlias { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();

    // S17 debug — remove after fix confirmed
    eprintln!("[invariant_solver] impl_target_names: {:?}", impl_target_names);
    for (idx, node) in ir.nodes.iter().enumerate() {
        if let NodeKind::Impl { for_struct, .. } = &node.kind {
            eprintln!("[invariant_solver] Impl node {} for_struct={:?} found={}", idx, for_struct, impl_target_names.contains(for_struct.as_str()));
        }
    }

    for (idx, node) in ir.nodes.iter().enumerate() {
        if let NodeKind::Impl { for_struct, .. } = &node.kind {
            if !impl_target_names.contains(for_struct.as_str()) {
                bail!("invariant_solver: Impl node {} references unknown struct {:?}", idx, for_struct);
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

fn node_kind_tag(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Crate { .. } => "Crate",
        NodeKind::Module { .. } => "Module",
        NodeKind::Struct { .. } => "Struct",
        NodeKind::Enum { .. } => "Enum",
        NodeKind::Trait { .. } => "Trait",
        NodeKind::Impl { .. } => "Impl",
        NodeKind::Function { .. } => "Function",
        NodeKind::Method { .. } => "Method",
        NodeKind::Const { .. } => "Const",
        NodeKind::Static { .. } => "Static",
        NodeKind::Use { .. } => "Use",
        NodeKind::TypeRef { .. } => "TypeRef",
        NodeKind::TypeAlias { .. } => "TypeAlias",
        NodeKind::Lifetime { .. } => "Lifetime",
        NodeKind::ExternCrate { .. } => "ExternCrate",
        NodeKind::MacroCall { .. } => "MacroCall",
    }
}

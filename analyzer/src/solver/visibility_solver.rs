//! Visibility Solver (S2) — enforce pub/private access rules.
//!
//! Variables:
//!   G_module = ir.module_graph       (Contains edges)
//!   G_name   = ir.name_graph         (Resolves edges: use-site -> definition)
//!   vis(v)   = visibility of node v
//!   mod(v)   = containing module of v (DFS on inv_module)
//!   anc(m,n) = m is an ancestor-or-equal of n in G_module
//!
//! Equations:
//!   visible(u, v) <=>
//!       vis(v) = Public
//!     ∨ vis(v) = PubCrate           (same crate — always true in single-crate IR)
//!     ∨ vis(v) = Private ∧ mod(u) = mod(v)
//!     ∨ vis(v) = PubSuper ∧ anc(parent(mod(v)), mod(u))
//!
//!   violation: (u, v, Resolves) ∈ G_name ∧ ¬visible(u, v)

use anyhow::Result;
use model::ir::{
    edge::EdgeKind,
    model_ir::ModelIR,
    node::{NodeId, NodeKind, Visibility},
};
use algorithms::graph::reachability::reachability;
use crate::solver::csr_to_adj;

pub fn solve(ir: &ModelIR) -> Result<()> {
    let n = ir.nodes.len();
    let name_v = ir.name_graph.vertex_count();
    let mod_v = ir.module_graph.vertex_count();
    if name_v == 0 || mod_v == 0 { return Ok(()); }

    // Build inv_module: child -> [parents]
    // Equation: inv[dst] += src for (src, dst, Contains) in G_module
    let fwd = csr_to_adj(&ir.module_graph);
    let mut inv: Vec<Vec<usize>> = vec![Vec::new(); mod_v.max(n)];
    for (src, nbrs) in fwd.iter().enumerate() {
        for &dst in nbrs {
            if dst < inv.len() { inv[dst].push(src); }
        }
    }

    // containing_module(idx): first Module ancestor in inv
    let containing_module = |start: usize| -> Option<usize> {
        if start >= inv.len() { return None; }
        let mut stack = vec![start];
        let mut seen = vec![false; inv.len()];
        while let Some(u) = stack.pop() {
            if seen[u] { continue; }
            seen[u] = true;
            if let Some(NodeKind::Module { .. }) = ir.nodes.get(u).map(|n| &n.kind) {
                return Some(u);
            }
            for &p in &inv[u] { if !seen[p] { stack.push(p); } }
        }
        None
    };

    // ancestor_or_eq(a, b): is a reachable from b via inv (i.e. a is above b)?
    // We use forward reachability on fwd: a can reach b => a is ancestor of b.
    let ancestor_or_eq = |a: usize, b: usize| -> bool {
        if a == b { return true; }
        if a >= fwd.len() { return false; }
        let reach = reachability(&fwd, &[a]);
        b < reach.len() && reach[b]
    };

    // parent_module(m): first entry in inv[m]
    let parent_module = |m: usize| -> Option<usize> { inv.get(m)?.first().copied() };

    let mut warnings: Vec<String> = Vec::new();

    // Check every Resolves edge: (use-site u) -> (definition v)
    for src_idx in 0..name_v {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
            if *edge != EdgeKind::Resolves { continue; }
            let dst_idx = dst_id.index();

            let vis = match ir.nodes.get(dst_idx).map(|n| &n.kind) {
                Some(NodeKind::Struct    { vis, .. }) => vis.clone(),
                Some(NodeKind::Function  { vis, .. }) => vis.clone(),
                Some(NodeKind::Method    { vis, .. }) => vis.clone(),
                Some(NodeKind::Trait     { vis, .. }) => vis.clone(),
                Some(NodeKind::TypeAlias { vis, .. }) => vis.clone(),
                _ => Visibility::Public, // Crate, Module, Use — always reachable
            };

            let ok = match &vis {
                Visibility::Public   => true,
                Visibility::PubCrate => true, // single-crate IR
                Visibility::Private  => {
                    containing_module(src_idx) == containing_module(dst_idx)
                }
                Visibility::PubSuper => {
                    if let (Some(sm), Some(dm)) =
                        (containing_module(src_idx), containing_module(dst_idx))
                    {
                        parent_module(dm)
                            .map(|p| ancestor_or_eq(p, sm))
                            .unwrap_or(false)
                    } else { false }
                }
                Visibility::PubIn(_) => true, // conservative: accept
            };

            if !ok {
                warnings.push(format!(
                    "visibility_solver: node {} accesses private item {} ({:?})",
                    src_idx, dst_idx, vis
                ));
            }
        }
    }

    for w in &warnings { eprintln!("WARN {}", w); }
    Ok(())
}

//! Name Provenance Solver (S6) — symbol origin tracking and shadow detection.
//!
//! Variables:
//!   G_name = ir.name_graph   (Renames, Resolves edges)
//!   origin(v) = root of Resolves chain from v
//!   shadow(u,v) <=> name(u)=name(v) ∧ mod(u)=mod(v) ∧ u≠v
//!
//! Equations:
//!   provenance(v) = DFS on Resolves edges until no outgoing Resolves
//!   shadow(u,v)   detected when two nodes in same module share a name

use anyhow::Result;
use model::ir::{
    edge::EdgeKind,
    model_ir::ModelIR,
    node::{NodeId, NodeKind},
};
use algorithms::graph::dfs::dfs;
use crate::solver::csr_to_adj;
use std::collections::HashMap;

fn node_name(kind: &NodeKind) -> Option<&str> {
    match kind {
        NodeKind::Function  { name, .. } => Some(name),
        NodeKind::Method    { name, .. } => Some(name),
        NodeKind::Struct    { name, .. } => Some(name),
        NodeKind::Trait     { name, .. } => Some(name),
        NodeKind::TypeAlias { name, .. } => Some(name),
        NodeKind::TypeRef   { name }     => Some(name),
        NodeKind::Use       { path, alias } => Some(alias.as_deref().unwrap_or(path.as_str())),
        _ => None,
    }
}

pub fn solve(ir: &ModelIR) -> Result<()> {
    let name_v = ir.name_graph.vertex_count();
    let mod_v  = ir.module_graph.vertex_count();
    if name_v == 0 || mod_v == 0 { return Ok(()); }

    // Build Resolves-only adjacency for provenance DFS.
    // Equation: resolves_adj[u] = { v | (u, v, Resolves) ∈ G_name }
    let mut resolves_adj: Vec<Vec<usize>> = vec![Vec::new(); name_v];
    for src_idx in 0..name_v {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
            if *edge == EdgeKind::Resolves {
                resolves_adj[src_idx].push(dst_id.index());
            }
        }
    }

    // Compute provenance (origin node) for each named node.
    // Equation: origin(v) = last node reachable from v via resolves_adj
    let mut origin: Vec<usize> = (0..name_v).collect(); // default: self
    for start in 0..name_v {
        let chain = dfs(&resolves_adj, start);
        if let Some(&last) = chain.last() {
            origin[start] = last;
        }
    }

    // Shadow detection: two nodes in same module with same name.
    // Equation: shadow(u,v) <=> mod(u)=mod(v) ∧ name(u)=name(v) ∧ u≠v
    let fwd = csr_to_adj(&ir.module_graph);
    let mut inv_mod: Vec<Vec<usize>> = vec![Vec::new(); fwd.len().max(ir.nodes.len())];
    for (src, nbrs) in fwd.iter().enumerate() {
        for &dst in nbrs {
            if dst < inv_mod.len() { inv_mod[dst].push(src); }
        }
    }

    let containing_module = |start: usize| -> Option<usize> {
        if start >= inv_mod.len() { return None; }
        let mut stack = vec![start];
        let mut seen = vec![false; inv_mod.len()];
        while let Some(u) = stack.pop() {
            if seen[u] { continue; }
            seen[u] = true;
            if let Some(NodeKind::Module { .. }) = ir.nodes.get(u).map(|n| &n.kind) {
                return Some(u);
            }
            for &p in &inv_mod[u] { if !seen[p] { stack.push(p); } }
        }
        None
    };

    // Group by (module, name)
    let mut by_mod_name: HashMap<(usize, &str), Vec<usize>> = HashMap::new();
    for idx in 0..ir.nodes.len() {
        if let Some(name) = node_name(&ir.nodes[idx].kind) {
            if let Some(m) = containing_module(idx) {
                by_mod_name.entry((m, name)).or_default().push(idx);
            }
        }
    }
    for ((m, name), indices) in &by_mod_name {
        if indices.len() > 1 {
            eprintln!(
                "WARN provenance_solver: name {:?} shadowed in module {} by nodes {:?}",
                name, m, indices
            );
        }
    }

    Ok(())
}

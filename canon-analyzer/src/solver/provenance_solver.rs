use crate::solver::csr_to_adj;
use algorithms::graph::dfs::dfs;
use anyhow::Result;
use canon::node::CanonNodeKind;
use canon::CanonIR;
use model::ir::{edge::EdgeKind, node::NodeId};
use std::collections::HashMap;

fn node_name<'a>(ir: &'a CanonIR, kind: &'a CanonNodeKind) -> Option<&'a str> {
    match kind {
        CanonNodeKind::Fn { name_id, .. }
        | CanonNodeKind::Struct { name_id, .. }
        | CanonNodeKind::Enum { name_id, .. }
        | CanonNodeKind::Trait { name_id, .. }
        | CanonNodeKind::TypeAlias { name_id, .. }
        | CanonNodeKind::TypeRef { name_id }
        | CanonNodeKind::Const { name_id, .. }
        | CanonNodeKind::Static { name_id, .. }
        | CanonNodeKind::ExternCrate { name_id, .. }
        | CanonNodeKind::Lifetime { name_id }
        | CanonNodeKind::GenericParam { name_id, .. }
        | CanonNodeKind::Param { name_id, .. }
        | CanonNodeKind::Variant { name_id, .. } => Some(ir.lookup_name(*name_id)),
        CanonNodeKind::Use { alias: Some(alias), .. } => Some(ir.lookup_name(*alias)),
        CanonNodeKind::Use { path_id, alias: None, .. } => Some(ir.lookup_path(*path_id)),
        _ => None,
    }
}

pub fn solve(ir: &CanonIR) -> Result<()> {
    let name_v = ir.name_graph.vertex_count();
    let mod_v = ir.module_graph.vertex_count();
    if name_v == 0 || mod_v == 0 {
        return Ok(());
    }

    let mut resolves_adj: Vec<Vec<usize>> = vec![Vec::new(); name_v];
    for (src_idx, slot) in resolves_adj.iter_mut().enumerate() {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
            if *edge == EdgeKind::Resolves {
                slot.push(dst_id.index());
            }
        }
    }

    let mut origin: Vec<usize> = (0..name_v).collect();
    for (start, slot) in origin.iter_mut().enumerate() {
        let chain = dfs(&resolves_adj, start);
        if let Some(&last) = chain.last() {
            *slot = last;
        }
    }

    let fwd = csr_to_adj(&ir.module_graph);
    let mut inv_mod: Vec<Vec<usize>> = vec![Vec::new(); fwd.len().max(ir.nodes.len())];
    for (src, nbrs) in fwd.iter().enumerate() {
        for &dst in nbrs {
            if dst < inv_mod.len() {
                inv_mod[dst].push(src);
            }
        }
    }

    let containing_module = |start: usize| -> Option<usize> {
        if start >= inv_mod.len() {
            return None;
        }
        let mut stack = vec![start];
        let mut seen = vec![false; inv_mod.len()];
        while let Some(u) = stack.pop() {
            if seen[u] {
                continue;
            }
            seen[u] = true;
            if let Some(CanonNodeKind::Module { .. }) = ir.nodes.get(u).map(|n| &n.kind) {
                return Some(u);
            }
            for &p in &inv_mod[u] {
                if !seen[p] {
                    stack.push(p);
                }
            }
        }
        None
    };

    let mut by_mod_name: HashMap<(usize, String), Vec<usize>> = HashMap::new();
    for idx in 0..ir.nodes.len() {
        if let Some(name) = node_name(ir, &ir.nodes[idx].kind) {
            if let Some(m) = containing_module(idx) {
                by_mod_name.entry((m, name.to_string())).or_default().push(idx);
            }
        }
    }
    for ((m, name), indices) in &by_mod_name {
        if indices.len() > 1 {
            eprintln!("WARN provenance_solver: name {:?} shadowed in module {} by nodes {:?}", name, m, indices);
        }
    }

    let _ = origin;
    Ok(())
}

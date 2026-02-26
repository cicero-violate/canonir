use crate::solver::csr_to_adj;
use anyhow::Result;
use canon::csr_graph::CsrGraph;
use canon::node::{CanonId, CanonNodeKind};
use canon::CanonIR;
use canon::{edge::EdgeKind, id::NodeId};
use std::collections::{HashMap, HashSet};

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.module_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }
    // ── Dedup pass ───────────────────────────────────────────────────────────
    // Collapse Use nodes that are structural duplicates within the same parent
    // module. Key = (parent_module_idx, path_id, alias, flags).
    {
        let old_v = ir.module_graph.vertex_count();
        let mut raw_edges: Vec<(u32, u32, EdgeKind)> = Vec::new();
        for src_idx in 0..old_v {
            let src_id = NodeId(src_idx as u32);
            for (dst_id, edge) in ir.module_graph.neighbours(src_id) {
                raw_edges.push((src_id.0, dst_id.0, edge.clone()));
            }
        }
        // (parent, path_id, alias_id, flags) -> already emitted
        let mut seen: HashSet<(u32, u32, u32, u32)> = HashSet::new();
        let mut deduped: Vec<(u32, u32, EdgeKind)> = Vec::new();
        for (src, dst, edge) in raw_edges {
            let is_dup = if matches!(edge, EdgeKind::Contains) {
                if let Some(CanonNodeKind::Use { path_id, alias, flags }) = ir.nodes.get(dst as usize).map(|n| &n.kind) {
                    let key = (src, path_id.0, alias.map(|a| a.0).unwrap_or(u32::MAX), *flags);
                    !seen.insert(key)
                } else {
                    false
                }
            } else {
                false
            };
            if !is_dup {
                deduped.push((src, dst, edge));
            }
        }
        let node_data: Vec<CanonId> = (0..ir.nodes.len() as u32).map(CanonId).collect();
        ir.module_graph = CsrGraph::from_edges(node_data, deduped);
    }

    let name_v = ir.name_graph.vertex_count();
    if name_v == 0 {
        return Ok(());
    }

    let crate_name: String =
        ir.nodes.iter().find_map(|n| if let CanonNodeKind::Crate { name_id, .. } = &n.kind { Some(ir.lookup_name(*name_id).to_string()) } else { None }).unwrap_or_else(|| "crate".to_string());

    let fwd = csr_to_adj(&ir.module_graph);
    let mut inv_mod: Vec<Vec<usize>> = vec![Vec::new(); v.max(ir.nodes.len())];
    for (src_idx, neighbours) in fwd.iter().enumerate() {
        for &dst_idx in neighbours {
            if dst_idx < inv_mod.len() {
                inv_mod[dst_idx].push(src_idx);
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

    let mut resolves_pairs: Vec<(usize, usize)> = Vec::new();
    for src_idx in 0..name_v {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
            if *edge == EdgeKind::Resolves {
                resolves_pairs.push((src_idx, dst_id.index()));
            }
        }
    }
    if resolves_pairs.is_empty() {
        return Ok(());
    }

    let mut seen: HashSet<(usize, String)> = HashSet::new();
    let mut injections: Vec<(usize, String)> = Vec::new();

    for (site_idx, def_idx) in resolves_pairs {
        let site_mod = match containing_module(site_idx) {
            Some(m) => m,
            None => continue,
        };
        let def_mod = match containing_module(def_idx) {
            Some(m) => m,
            None => continue,
        };
        if site_mod == def_mod {
            continue;
        }

        let def_name = node_display_name(ir, def_idx);
        let def_mod_path = match ir.nodes.get(def_mod).map(|n| &n.kind) {
            Some(CanonNodeKind::Module { path_id, .. }) => ir.lookup_path(*path_id).to_string(),
            _ => continue,
        };

        let mod_path_stripped = def_mod_path.strip_prefix("crate").unwrap_or(&def_mod_path);
        let full_path = format!("{}{}::{}", crate_name, mod_path_stripped, def_name);
        let key = (site_mod, full_path.clone());
        if seen.insert(key) {
            injections.push((site_mod, full_path));
        }
    }
    if injections.is_empty() {
        return Ok(());
    }

    let old_v = ir.module_graph.vertex_count();
    let mut all_edges: Vec<(u32, u32, EdgeKind)> = Vec::new();
    for src_idx in 0..old_v {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.module_graph.neighbours(src_id) {
            all_edges.push((src_id.0, dst_id.0, edge.clone()));
        }
    }

    for (site_mod, full_path) in injections {
        let path_id = ir.intern_path(&full_path);
        let use_id = ir.push_node(CanonNodeKind::Use { path_id, alias: None, flags: 0 });
        all_edges.push((site_mod as u32, use_id.0, EdgeKind::Contains));
    }

    let node_data: Vec<CanonId> = (0..ir.nodes.len() as u32).map(CanonId).collect();
    ir.module_graph = CsrGraph::from_edges(node_data, all_edges);

    Ok(())
}

fn node_display_name(ir: &CanonIR, idx: usize) -> String {
    match ir.nodes.get(idx).map(|n| &n.kind) {
        Some(CanonNodeKind::Fn { name_id, .. })
        | Some(CanonNodeKind::Struct { name_id, .. })
        | Some(CanonNodeKind::Enum { name_id, .. })
        | Some(CanonNodeKind::Trait { name_id, .. })
        | Some(CanonNodeKind::TypeAlias { name_id, .. })
        | Some(CanonNodeKind::TypeRef { name_id })
        | Some(CanonNodeKind::Const { name_id, .. })
        | Some(CanonNodeKind::Static { name_id, .. })
        | Some(CanonNodeKind::ExternCrate { name_id, .. })
        | Some(CanonNodeKind::Lifetime { name_id })
        | Some(CanonNodeKind::GenericParam { name_id, .. })
        | Some(CanonNodeKind::Param { name_id, .. })
        | Some(CanonNodeKind::Variant { name_id, .. }) => ir.lookup_name(*name_id).to_string(),
        Some(CanonNodeKind::Module { path_id, .. }) => ir.lookup_path(*path_id).to_string(),
        Some(CanonNodeKind::Use { path_id, .. }) => ir.lookup_path(*path_id).to_string(),
        _ => format!("node_{}", idx),
    }
}

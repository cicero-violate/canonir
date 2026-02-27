use crate::solver::csr_to_adj;
use anyhow::Result;
use canon::csr_graph::CsrGraph;
use canon::node::{flags, CanonId, CanonNodeKind};
use canon::CanonIR;
use canon::{edge::EdgeKind, id::NodeId};
use std::collections::{HashMap, HashSet};

fn containing_module(ir: &CanonIR, inv_mod: &[Vec<usize>], start: usize) -> Option<usize> {
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
}

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.module_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }
    // ── Dedup pass ───────────────────────────────────────────────────────────
    // Collapse Use nodes that are structural duplicates within the same parent
    // module. Key = (parent_module_idx, path_id, alias, flags, target).
    {
        let old_v = ir.module_graph.vertex_count();
        let mut raw_edges: Vec<(u32, u32, EdgeKind)> = Vec::new();
        for src_idx in 0..old_v {
            let src_id = NodeId(src_idx as u32);
            for (dst_id, edge) in ir.module_graph.neighbours(src_id) {
                raw_edges.push((src_id.0, dst_id.0, edge.clone()));
            }
        }
        // (parent, path_id, alias_id, flags, target_id) -> already emitted
        let mut seen: HashSet<(u32, u32, u32, u32, u32)> = HashSet::new();
        let mut deduped: Vec<(u32, u32, EdgeKind)> = Vec::new();
        for (src, dst, edge) in raw_edges {
            let is_dup = if matches!(edge, EdgeKind::Contains) {
                if let Some(CanonNodeKind::Use { path_id, alias, flags, target }) = ir.nodes.get(dst as usize).map(|n| &n.kind) {
                    let key = (src, path_id.0, alias.map(|a| a.0).unwrap_or(u32::MAX), *flags, target.map(|t| t.0).unwrap_or(u32::MAX));
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

    let mut resolves_by_site: HashMap<usize, Vec<usize>> = HashMap::new();
    for src_idx in 0..name_v {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
            if *edge == EdgeKind::Resolves {
                resolves_by_site.entry(src_idx).or_default().push(dst_id.index());
            }
        }
    }
    if resolves_by_site.is_empty() {
        return Ok(());
    }

    for (site_idx, defs) in &mut resolves_by_site {
        defs.sort_unstable();
        defs.dedup();
        if let Some(CanonNodeKind::Use { target, .. }) = ir.nodes.get_mut(*site_idx).map(|n| &mut n.kind) {
            *target = if defs.len() == 1 { Some(CanonId(defs[0] as u32)) } else { None };
        }
    }

    let mut seen: HashSet<(usize, String)> = HashSet::new();
    let mut injections: Vec<(usize, String, CanonId)> = Vec::new();

    // Pre-populate seen with existing Use nodes per module to avoid
    // injecting duplicates of Use nodes already present in the IR.
    for src_idx in 0..ir.module_graph.vertex_count() {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.module_graph.neighbours(src_id) {
            if !matches!(edge, EdgeKind::Contains) {
                continue;
            }
            if let Some(CanonNodeKind::Use { path_id, .. }) = ir.nodes.get(dst_id.index()).map(|n| &n.kind) {
                let path_str = ir.lookup_path(*path_id).to_string();
                seen.insert((src_idx, path_str));
            }
        }
    }

    for (site_idx, defs) in resolves_by_site {
        if defs.len() != 1 {
            continue;
        }
        let def_idx = defs[0];
        if let Some(CanonNodeKind::Use { flags: use_flags, .. }) = ir.nodes.get(def_idx).map(|n| &n.kind) {
            let vis_mask = flags::PUB | flags::PUB_CRATE | flags::PUB_SUPER;
            if (*use_flags & vis_mask) == 0 {
                continue;
            }
        }
        let site_mod = match containing_module(ir, &inv_mod, site_idx) {
            Some(m) => m,
            None => continue,
        };
        let def_mod = match containing_module(ir, &inv_mod, def_idx) {
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

        // If def_idx itself is a Module, use its full path directly.
        // Otherwise build path as def_mod_path::def_name.
        let full_path = match ir.nodes.get(def_idx).map(|n| &n.kind) {
            Some(CanonNodeKind::Module { path_id, .. }) => {
                ir.lookup_path(*path_id).to_string()
            }
            _ => {
                let mod_path_stripped =
                    def_mod_path.strip_prefix("crate").unwrap_or(&def_mod_path);
                format!("crate{}::{}", mod_path_stripped, def_name)
            }
        };
        let key = (site_mod, full_path.clone());
        if seen.insert(key) {
            injections.push((site_mod, full_path, CanonId(def_idx as u32)));
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

    for (site_mod, full_path, target) in injections {
        let path_id = ir.intern_path(&full_path);
        let use_id = ir.push_node(CanonNodeKind::Use { path_id, alias: None, flags: 0, target: Some(target) });
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
        Some(CanonNodeKind::Module { path_id, .. }) => {
            let full = ir.lookup_path(*path_id);
            full.rsplit("::").next().unwrap_or(full).to_string()
        }
        Some(CanonNodeKind::Use { path_id, .. }) => ir.lookup_path(*path_id).to_string(),
        _ => format!("node_{}", idx),
    }
}

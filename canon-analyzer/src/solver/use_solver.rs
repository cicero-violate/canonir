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

    let fwd = csr_to_adj(&ir.module_graph);
    let mut use_parent_module: HashMap<usize, usize> = HashMap::new();
    for (src_idx, neighbours) in fwd.iter().enumerate() {
        for &dst_idx in neighbours {
            if let Some(CanonNodeKind::Use { .. }) = ir.nodes.get(dst_idx).map(|n| &n.kind) {
                use_parent_module.entry(dst_idx).or_insert(src_idx);
            }
        }
    }

    let mut resolves_by_site: HashMap<usize, Vec<usize>> = HashMap::new();
    for src_idx in 0..name_v {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
            if *edge == EdgeKind::Resolves || *edge == EdgeKind::Reexports {
                resolves_by_site.entry(src_idx).or_default().push(dst_id.index());
            }
        }
    }

    for (site_idx, defs) in &mut resolves_by_site {
        defs.sort_unstable();
        defs.dedup();
        if let Some(CanonNodeKind::Use { target, .. }) = ir.nodes.get_mut(*site_idx).map(|n| &mut n.kind) {
            *target = if defs.len() == 1 { Some(CanonId(defs[0] as u32)) } else { None };
        }
    }

    for (site_idx, parent_mod) in use_parent_module {
        if resolves_by_site.contains_key(&site_idx) {
            continue;
        }
        if let Some(CanonNodeKind::Use { path_id, .. }) = ir.nodes.get(site_idx).map(|n| &n.kind) {
            eprintln!(
                "WARN use_solver: unresolved use at node {} in module {} path {}",
                site_idx,
                parent_mod,
                ir.lookup_path(*path_id)
            );
        } else {
            eprintln!("WARN use_solver: unresolved use-like node {}", site_idx);
        }
    }

    Ok(())
}

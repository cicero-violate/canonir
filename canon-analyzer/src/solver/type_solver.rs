use crate::solver::csr_to_adj;
use algorithms::graph::scc::kosaraju_scc;
use anyhow::Result;
use canon::edge::EdgeKind;
use canon::id::NodeId;
use canon::node::{CanonId, CanonNodeKind, TypeKind};
use canon::CanonIR;
use std::collections::{HashMap, HashSet};

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.type_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.type_graph);
    let sccs = kosaraju_scc(&adj);
    for scc in sccs.iter().filter(|s| s.len() > 1) {
        let _ = scc;
    }

    derive_instantiates_edges(ir);

    Ok(())
}

fn derive_instantiates_edges(ir: &mut CanonIR) {
    let local_type_by_path = collect_local_type_paths(ir);

    let mut existing: HashSet<(u32, u32, &'static str)> = HashSet::new();
    for src in 0..ir.type_graph.vertex_count() {
        for (dst, edge) in ir.type_graph.neighbours(NodeId(src as u32)) {
            if *edge == EdgeKind::Instantiates {
                existing.insert((src as u32, dst.0, "inst"));
            }
        }
    }

    let mut new_edges: Vec<(u32, u32, EdgeKind)> = Vec::new();
    for n in &ir.nodes {
        let (base, args) = match &n.kind {
            CanonNodeKind::Type { kind: TypeKind::Applied { base, args } } => (*base, args),
            _ => continue,
        };

        if let Some(def_id) = resolve_applied_base_def(ir, base, &local_type_by_path) {
            if existing.insert((n.id.0, def_id, "inst")) {
                new_edges.push((n.id.0, def_id, EdgeKind::Instantiates));
            }
        }

        for arg in args {
            let arg_id = arg.0;
            if existing.insert((n.id.0, arg_id, "inst")) {
                new_edges.push((n.id.0, arg_id, EdgeKind::Instantiates));
            }
        }
    }

    if new_edges.is_empty() {
        return;
    }

    let mut all_edges: Vec<(u32, u32, EdgeKind)> = Vec::new();
    for src in 0..ir.type_graph.vertex_count() {
        for (dst, edge) in ir.type_graph.neighbours(NodeId(src as u32)) {
            all_edges.push((src as u32, dst.0, edge.clone()));
        }
    }
    all_edges.extend(new_edges);

    let node_ids: Vec<CanonId> = (0..ir.nodes.len() as u32).map(CanonId).collect();
    ir.type_graph = canon::csr_graph::CsrGraph::from_edges(node_ids, all_edges);
}

fn collect_local_type_paths(ir: &CanonIR) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    for src in 0..ir.module_graph.vertex_count() {
        let Some(CanonNodeKind::Module { path_id, .. }) = ir.nodes.get(src).map(|n| &n.kind) else {
            continue;
        };
        let module_path = ir.lookup_path(*path_id).to_string();
        for (dst_id, edge) in ir.module_graph.neighbours(NodeId(src as u32)) {
            if *edge != EdgeKind::Contains {
                continue;
            }
            let Some(kind) = ir.nodes.get(dst_id.index()).map(|n| &n.kind) else {
                continue;
            };
            let Some(name_id) = (match kind {
                CanonNodeKind::Struct { name_id, .. } | CanonNodeKind::Enum { name_id, .. } | CanonNodeKind::Trait { name_id, .. } | CanonNodeKind::TypeAlias { name_id, .. } => Some(*name_id),
                _ => None,
            }) else {
                continue;
            };
            let full = format!("{module_path}::{}", ir.lookup_name(name_id));
            out.entry(full).or_insert(dst_id.0);
        }
    }
    out
}

fn resolve_applied_base_def(ir: &CanonIR, base: CanonId, local_type_by_path: &HashMap<String, u32>) -> Option<u32> {
    let CanonNodeKind::Type { kind } = &ir.node(base).kind else {
        return None;
    };
    match kind {
        TypeKind::Adt(id) => Some(id.0),
        TypeKind::Extern(path_id) | TypeKind::Unresolved(path_id) => local_type_by_path.get(ir.lookup_path(*path_id)).copied(),
        _ => None,
    }
}

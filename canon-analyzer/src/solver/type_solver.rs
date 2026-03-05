use crate::solver::csr_to_adj;
#[cfg(feature = "cuda")]
use crate::solver::gpu_algorithms::ac3_gpu_apply;
use crate::solver::gpu_algorithms::kosaraju_scc;
#[cfg(feature = "cuda")]
use algorithms::constraints::ac3::{ConstraintGraph, Domain};
use anyhow::Result;
use canon::edge::EdgeKind;
use canon::id::NodeId;
#[cfg(feature = "cuda")]
use canon::ir::TypeKey;
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

    #[cfg(feature = "cuda")]
    {
        let (domains, graph, var_to_node) = build_type_constraint_graph(ir);
        if !graph.constraints.is_empty() {
            let pruned = ac3_gpu_apply(&domains, &graph).unwrap_or_else(|| {
                eprintln!("WARN type_solver: ac3_gpu_apply validation failed; skipping pruning");
                domains.clone()
            });
            for (var_idx, dom) in pruned.iter().enumerate() {
                if dom.is_empty() {
                    let node_id = var_to_node.get(var_idx).copied().unwrap_or_default();
                    eprintln!("WARN type_solver: empty type domain for node {}", node_id);
                }
            }
            apply_pruned_type_domains(ir, &pruned, &var_to_node);
        }
    }

    Ok(())
}

#[cfg(feature = "cuda")]
fn build_type_constraint_graph(ir: &CanonIR) -> (Vec<Domain>, ConstraintGraph, Vec<usize>) {
    let v = ir.type_graph.vertex_count();
    let mut type_nodes = Vec::new();
    let mut node_to_var = vec![None; v];
    for idx in 0..v {
        if matches!(ir.nodes.get(idx).map(|n| &n.kind), Some(CanonNodeKind::Type { .. })) {
            node_to_var[idx] = Some(type_nodes.len());
            type_nodes.push(idx);
        }
    }

    let mut concrete_types = Vec::new();
    for &node_idx in &type_nodes {
        let Some(CanonNodeKind::Type { kind }) = ir.nodes.get(node_idx).map(|n| &n.kind) else {
            continue;
        };
        if is_concrete_type(kind) {
            concrete_types.push(node_idx as i32);
        }
    }
    let fallback_domain: Vec<i32> = if concrete_types.is_empty() { type_nodes.iter().map(|&i| i as i32).collect() } else { concrete_types.clone() };

    let mut domains = Vec::with_capacity(type_nodes.len());
    for &node_idx in &type_nodes {
        let Some(CanonNodeKind::Type { kind }) = ir.nodes.get(node_idx).map(|n| &n.kind) else {
            continue;
        };
        let dom = if is_concrete_type(kind) { vec![node_idx as i32] } else { fallback_domain.clone() };
        domains.push(dom);
    }

    let mut graph = ConstraintGraph::default();
    for src in 0..v {
        let Some(src_var) = node_to_var.get(src).and_then(|v| *v) else {
            continue;
        };
        for (dst, edge) in ir.type_graph.neighbours(NodeId(src as u32)) {
            let dst_idx = dst.index();
            let Some(dst_var) = node_to_var.get(dst_idx).and_then(|v| *v) else {
                continue;
            };
            match edge {
                EdgeKind::TypeUnifies | EdgeKind::Instantiates => {
                    graph.add_constraint(src_var, dst_var, |a, b| a == b);
                    graph.add_constraint(dst_var, src_var, |a, b| a == b);
                }
                _ => {}
            }
        }
    }

    (domains, graph, type_nodes)
}

#[cfg(feature = "cuda")]
fn is_concrete_type(kind: &TypeKind) -> bool {
    !matches!(kind, TypeKind::Param(_) | TypeKind::Extern(_) | TypeKind::Unresolved(_) | TypeKind::TypeRef { .. })
}

#[cfg(feature = "cuda")]
fn apply_pruned_type_domains(ir: &mut CanonIR, pruned: &[Domain], var_to_node: &[usize]) {
    for (var_idx, dom) in pruned.iter().enumerate() {
        if dom.len() != 1 {
            continue;
        }
        let resolved_id = dom[0] as u32;
        let Some(&node_idx) = var_to_node.get(var_idx) else {
            continue;
        };
        let Some(node) = ir.nodes.get_mut(node_idx) else {
            continue;
        };
        let CanonNodeKind::Type { kind } = &mut node.kind else {
            continue;
        };
        if matches!(kind, TypeKind::Unresolved(_) | TypeKind::Param(_)) {
            let old_kind = kind.clone();
            *kind = TypeKind::Adt(CanonId(resolved_id));
            if let Some(existing) = ir.type_index.get(&TypeKey(old_kind.clone())) {
                if *existing == CanonId(node_idx as u32) {
                    ir.type_index.remove(&TypeKey(old_kind));
                }
            }
            ir.type_index.insert(TypeKey(kind.clone()), CanonId(node_idx as u32));
        }
    }
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

//! Use-declaration solver — Gap 1 & 3.
//!
//! Variables:
//!   G_name   = ir.name_graph    (Resolves edges: use-site -> definition)
//!   G_module = ir.module_graph  (Contains edges: module -> item)
//!   inv_mod  : Vec<Vec<usize>>  — inverted module_graph: child -> [parents]
//!   mod_path : usize -> String  — Module.path for each Module node
//!   item_name: usize -> String  — name() of a definition node
//!
//!   crate_name = NodeKind::Crate.name
//!   is_bin_mod(m) = m.file == "src/main.rs" (binary, not reachable via lib.rs)
//!   use_prefix(site_mod) = crate_name  if is_bin_mod(site_mod)
//!                        = "crate"     otherwise
//!   full_path(v, site_mod) =
//!     use_prefix(site_mod) + mod_path(containing_module(v)).strip_prefix("crate") + "::" + item_name(v)
//!
//! Equations:
//!   inv_mod[child] = { parent | (parent, child, Contains) ∈ G_module }
//!   containing_module(n) = first Module ancestor via DFS on inv_mod from n
//!   full_path(v)         = mod_path(containing_module(v)) ++ "::" ++ item_name(v)
//!
//!   for (u, v, Resolves) in G_name:
//!     site_mod = containing_module(u)
//!     def_mod  = containing_module(v)
//!     if site_mod != def_mod:
//!       inject NodeKind::Use { path: full_path(v), alias: None }
//!       inject Contains edge: site_mod -> Use node
//!       (deduplicated by (site_mod, full_path))
//!
//! Algorithm: DFS (algorithms::graph::dfs) on inv_mod to walk upward.

use std::collections::HashSet;
use anyhow::Result;
use algorithms::graph::dfs::dfs;
use model::ir::{
    edge::EdgeKind,
    model_ir::ModelIR,
    node::{Node, NodeId, NodeKind},
};
use crate::graph::module_graph::ModuleGraphBuilder;
use crate::solver::csr_to_adj;

pub fn solve(ir: &mut ModelIR) -> Result<()> {
    let v = ir.module_graph.vertex_count();
    if v == 0 { return Ok(()); }
    let name_v = ir.name_graph.vertex_count();
    if name_v == 0 { return Ok(()); }

    // ── Phase A: pure read — build all data structures from ir ──────────────
    // All reads happen here; no mutation until Phase B.

    // Crate name for binary-module use paths.
    // Equation: crate_name = first NodeKind::Crate node's name field.
    let crate_name: String = ir.nodes.iter().find_map(|n| {
        if let NodeKind::Crate { name, .. } = &n.kind { Some(name.clone()) } else { None }
    }).unwrap_or_else(|| "crate".to_string());

    // Binary module set: modules whose file is src/main.rs or does not live
    // under the lib.rs root. We detect by file name ending in "main.rs".
    // Equation: bin_mods = { idx | snap[idx] == Module && file ends_with "main.rs" }
    let bin_mod_set: HashSet<usize> = ir.nodes.iter().enumerate().filter_map(|(idx, n)| {
        if let NodeKind::Module { file, .. } = &n.kind {
            if file.ends_with("main.rs") { return Some(idx); }
        }
        None
    }).collect();

    // inv_mod[child_idx] = [parent_idx, ...]
    // Equation: inv_mod[dst] += src  for (src, dst, _) in fwd_module_edges
    let fwd = csr_to_adj(&ir.module_graph);
    let mut inv_mod: Vec<Vec<usize>> = vec![Vec::new(); v];
    for (src_idx, neighbours) in fwd.iter().enumerate() {
        for &dst_idx in neighbours {
            if dst_idx < v {
                inv_mod[dst_idx].push(src_idx);
            }
        }
    }

    // Snapshot node kinds for read-only access in closures below.
    // (avoids borrowing ir.nodes inside closures while also mutating it later)
    #[derive(Clone)]
    enum SnapKind {
        Module { path: String },
        Named  { name: String },
        Other,
    }
    let snap: Vec<SnapKind> = ir.nodes.iter().map(|n| match &n.kind {
        NodeKind::Module    { path, .. } => SnapKind::Module { path: path.clone() },
        NodeKind::Function  { name, .. } => SnapKind::Named  { name: name.clone() },
        NodeKind::Method    { name, .. } => SnapKind::Named  { name: name.clone() },
        NodeKind::Struct    { name, .. } => SnapKind::Named  { name: name.clone() },
        NodeKind::Trait     { name, .. } => SnapKind::Named  { name: name.clone() },
        NodeKind::TypeAlias { name, .. } => SnapKind::Named  { name: name.clone() },
        NodeKind::TypeRef   { name }     => SnapKind::Named  { name: name.clone() },
        _                                => SnapKind::Other,
    }).collect();

    // containing_module(n):
    //   DFS upward on inv_mod from n; return first index whose snap is Module.
    //   inv_mod is a DAG (forest), so DFS terminates.
    let containing_module = |start: usize| -> Option<usize> {
        if start >= inv_mod.len() { return None; }
        let visited = dfs(&inv_mod, start);
        for node_idx in visited {
            if let Some(SnapKind::Module { .. }) = snap.get(node_idx) {
                return Some(node_idx);
            }
        }
        None
    };

    // Collect Resolves pairs from name_graph.
    let mut resolves_pairs: Vec<(usize, usize)> = Vec::new();
    for src_idx in 0..name_v {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
            if *edge == EdgeKind::Resolves {
                resolves_pairs.push((src_idx, dst_id.index()));
            }
        }
    }
    if resolves_pairs.is_empty() { return Ok(()); }

    // Compute injections: Vec<(site_mod_idx, full_path)>
    // Equation:
    //   full_path(v) = mod_path(containing_module(v)) ++ "::" ++ item_name(v)
    let mut seen: HashSet<(usize, String)> = HashSet::new();
    let mut injections: Vec<(usize, String)> = Vec::new(); // (site_mod, full_path)

    for (site_idx, def_idx) in resolves_pairs {
        let site_mod = match containing_module(site_idx) { Some(m) => m, None => continue };
        let def_mod  = match containing_module(def_idx)  { Some(m) => m, None => continue };
        if site_mod == def_mod { continue; }

        let def_name = match snap.get(def_idx) {
            Some(SnapKind::Named  { name }) => name.clone(),
            Some(SnapKind::Module { path }) => path.clone(),
            _ => continue,
        };
        let def_mod_path = match snap.get(def_mod) {
            Some(SnapKind::Module { path }) => path.clone(),
            _ => continue,
        };
        // Equation:
        //   use_prefix(site_mod) = crate_name  if site_mod ∈ bin_mods
        //                        = "crate"     otherwise
        //   full_path = use_prefix + mod_path.strip_prefix("crate") + "::" + def_name
        let prefix = if bin_mod_set.contains(&site_mod) { crate_name.as_str() } else { "crate" };
        let mod_path_stripped = def_mod_path.strip_prefix("crate").unwrap_or(&def_mod_path);
        let full_path = format!("{}{}::{}", prefix, mod_path_stripped, def_name);
        let key = (site_mod, full_path.clone());
        if seen.contains(&key) { continue; }
        seen.insert(key);
        injections.push((site_mod, full_path));
    }
    if injections.is_empty() { return Ok(()); }

    // ── Phase B: mutation — inject Use nodes, rebuild module_graph ───────────
    // No closures borrow ir past this point.

    // Collect existing module_graph edges before rebuilding.
    let old_v = ir.module_graph.vertex_count();
    let mut all_edges: Vec<(u32, u32, EdgeKind)> = Vec::new();
    for src_idx in 0..old_v {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.module_graph.neighbours(src_id) {
            all_edges.push((src_id.0, dst_id.0, edge.clone()));
        }
    }

    // Inject Use nodes into arena and record new Contains edges.
    // Equation:
    //   for each (site_mod, full_path) in injections:
    //     use_id = |ir.nodes|
    //     ir.nodes[use_id] = Node { Use { path: full_path } }
    //     new_edge: (site_mod, use_id, Contains)
    for (site_mod, full_path) in injections {
        let use_id = ir.nodes.len() as u32;
        ir.nodes.push(Node {
            id: NodeId(use_id),
            kind: NodeKind::Use { path: full_path, alias: None },
            span: None,
        });
        all_edges.push((site_mod as u32, use_id, EdgeKind::Contains));
    }

    // Rebuild module_graph with all edges (old + new).
    // Equation:
    //   E'_module = E_module ∪ new_edges
    //   G'_module = CsrGraph::from_edges(new_v, E'_module)
    let new_v = ir.nodes.len();
    let mut builder = ModuleGraphBuilder::new(new_v);
    for (src, dst, kind) in all_edges {
        match kind {
            EdgeKind::Contains | EdgeKind::ImplFor => {
                builder.add_contains(NodeId(src), NodeId(dst));
            }
            _ => {}
        }
    }
    ir.module_graph = builder.build();

    Ok(())
}

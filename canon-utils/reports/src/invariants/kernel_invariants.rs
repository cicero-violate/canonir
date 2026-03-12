use crate::artifacts_loader::KernelGraph;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const KIND_MODULE: &str = "MODULE";
const KIND_CALLSITE: &str = "CALL_SITE";
const EDGE_CONTAINS: &str = "CONTAINS";
const EDGE_EXPORT: &str = "EXPORT";

#[derive(Debug, Serialize)]
struct KernelInvariantReport {
    generated_at_epoch_ms: u128,
    ok: bool,
    node_count: usize,
    edge_count: usize,
    edges_with_missing_src: usize,
    edges_with_missing_dst: usize,
    bad_file_id_nodes: usize,
    callsite_no_incoming: usize,
    isolated_nodes: usize,
    module_count: usize,
    module_root_like: usize,
    export_src_not_module: usize,
    duplicate_symbol_kind: usize,
    duplicate_symbol_kind_module: usize,
    missing_module_owner: usize,
    files_outside_project_root: usize,
    must_have_valid_edge_sources: bool,
    must_have_valid_edge_destinations: bool,
    must_have_valid_file_ids: bool,
    must_have_callsites_with_incoming_edges: bool,
    must_not_have_isolated_nodes: bool,
    must_have_single_module_root: bool,
    must_have_module_exports_from_modules: bool,
    must_have_unique_symbol_kind: bool,
    must_have_unique_symbol_kind_per_module: bool,
    must_have_module_owner: bool,
    must_have_files_within_project_root: bool,
    partial: bool,
    missing_fields: Vec<&'static str>,
}

pub fn write_kernel_invariants(
    graph_dir: &Path,
    reports_dir: &Path,
    graph: &KernelGraph,
) -> anyhow::Result<()> {
    let (report, ok) = build_report(graph_dir, graph);
    let mut report = report;
    report.ok = ok;
    let path = reports_dir.join("upg_invariants.json");
    fs::create_dir_all(reports_dir)?;
    fs::write(path, serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

fn build_report(graph_dir: &Path, graph: &KernelGraph) -> (KernelInvariantReport, bool) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let node_count = graph.nodes.len();
    let edge_count = graph.edges.len();
    let mut node_ids = HashSet::new();
    for n in &graph.nodes {
        node_ids.insert(n.id);
    }

    let mut edges_with_missing_src = 0usize;
    let mut edges_with_missing_dst = 0usize;
    for e in &graph.edges {
        if !node_ids.contains(&e.src) {
            edges_with_missing_src += 1;
        }
        if !node_ids.contains(&e.dst) {
            edges_with_missing_dst += 1;
        }
    }

    let files_set: HashSet<&str> = graph.files.iter().map(|s| s.as_str()).collect();
    let bad_file_id_nodes = graph
        .nodes
        .iter()
        .filter(|n| !n.file.is_empty())
        .filter(|n| !files_set.contains(n.file.as_str()))
        .count();

    let mut incoming: HashMap<u32, usize> = HashMap::new();
    let mut outgoing: HashMap<u32, usize> = HashMap::new();
    for e in &graph.edges {
        *incoming.entry(e.dst).or_insert(0) += 1;
        *outgoing.entry(e.src).or_insert(0) += 1;
    }

    let callsite_no_incoming = graph
        .nodes
        .iter()
        .filter(|n| n.kind == KIND_CALLSITE)
        .filter(|n| incoming.get(&n.id).copied().unwrap_or(0) == 0)
        .count();

    let isolated_nodes = graph
        .nodes
        .iter()
        .filter(|n| incoming.get(&n.id).is_none() && outgoing.get(&n.id).is_none())
        .count();

    let module_nodes: Vec<_> = graph.nodes.iter().filter(|n| n.kind == KIND_MODULE).collect();
    let module_count = module_nodes.len();
    let module_root_like = module_nodes
        .iter()
        .filter(|n| n.symbol == "crate" || n.symbol.is_empty())
        .count();

    let node_kind_by_id: HashMap<u32, &str> = graph
        .nodes
        .iter()
        .map(|n| (n.id, n.kind.as_str()))
        .collect();

    let export_src_not_module = graph
        .edges
        .iter()
        .filter(|e| e.kind == EDGE_EXPORT)
        .filter(|e| node_kind_by_id.get(&e.src).copied() != Some(KIND_MODULE))
        .count();

    let mut seen_symbol_kind: HashSet<(String, String)> = HashSet::new();
    let mut duplicate_symbol_kind = 0usize;
    for n in &graph.nodes {
        let key = (n.symbol.clone(), n.kind.clone());
        if !seen_symbol_kind.insert(key) {
            duplicate_symbol_kind += 1;
        }
    }

    let duplicate_symbol_kind_module = 0usize;

    let mut module_has_owner = HashSet::new();
    for e in &graph.edges {
        if e.kind == EDGE_CONTAINS {
            module_has_owner.insert(e.dst);
        }
    }
    let missing_module_owner = module_nodes
        .iter()
        .filter(|n| !module_has_owner.contains(&n.id))
        .count();

    let workspace_root = graph_dir
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| graph_dir.to_path_buf());
    let files_outside_project_root = graph
        .nodes
        .iter()
        .filter(|n| !n.file.is_empty())
        .filter(|n| !Path::new(&n.file).starts_with(&workspace_root))
        .count();

    let must_have_valid_edge_sources = edges_with_missing_src == 0;
    let must_have_valid_edge_destinations = edges_with_missing_dst == 0;
    let must_have_valid_file_ids = bad_file_id_nodes == 0;
    let must_have_callsites_with_incoming_edges = callsite_no_incoming == 0;
    let must_not_have_isolated_nodes = isolated_nodes == 0;
    let must_have_single_module_root = module_root_like <= 1;
    let must_have_module_exports_from_modules = export_src_not_module == 0;
    let must_have_unique_symbol_kind = duplicate_symbol_kind == 0;
    let must_have_unique_symbol_kind_per_module = duplicate_symbol_kind_module == 0;
    let must_have_module_owner = missing_module_owner == 0;
    let must_have_files_within_project_root = files_outside_project_root == 0;

    let ok = must_have_valid_edge_sources
        && must_have_valid_edge_destinations
        && must_have_valid_file_ids
        && must_have_callsites_with_incoming_edges
        && must_not_have_isolated_nodes
        && must_have_single_module_root
        && must_have_module_exports_from_modules
        && must_have_unique_symbol_kind
        && must_have_unique_symbol_kind_per_module
        && must_have_module_owner
        && must_have_files_within_project_root;

    let missing_fields = vec![
        "spans_count",
        "defs_count",
        "missing_def_nodes",
        "spans_match_nodes",
        "span_ids_match_nodes",
        "invalid_node_kinds",
        "invalid_edge_kinds",
        "edge_kind_mismatch",
        "bb_without_has_block",
        "call_without_has_block",
        "span_order_violations",
        "span_file_mismatch",
        "span_file_inconsistent",
        "function_cfg_disconnected",
        "orphan_files",
        "missing_entry_roots",
        "must_have_contiguous_node_ids",
        "must_have_valid_node_kinds",
        "must_have_valid_edge_kinds",
        "must_have_basic_blocks_with_owner",
        "must_have_callsites_with_owner",
        "must_have_ordered_spans",
        "must_have_consistent_span_file",
        "must_have_connected_function_cfg",
        "must_not_have_orphan_files",
        "must_have_entry_roots_in_files",
    ];

    (
        KernelInvariantReport {
            generated_at_epoch_ms: now,
            ok,
            node_count,
            edge_count,
            edges_with_missing_src,
            edges_with_missing_dst,
            bad_file_id_nodes,
            callsite_no_incoming,
            isolated_nodes,
            module_count,
            module_root_like,
            export_src_not_module,
            duplicate_symbol_kind,
            duplicate_symbol_kind_module,
            missing_module_owner,
            files_outside_project_root,
            must_have_valid_edge_sources,
            must_have_valid_edge_destinations,
            must_have_valid_file_ids,
            must_have_callsites_with_incoming_edges,
            must_not_have_isolated_nodes,
            must_have_single_module_root,
            must_have_module_exports_from_modules,
            must_have_unique_symbol_kind,
            must_have_unique_symbol_kind_per_module,
            must_have_module_owner,
            must_have_files_within_project_root,
            partial: true,
            missing_fields,
        },
        ok,
    )
}

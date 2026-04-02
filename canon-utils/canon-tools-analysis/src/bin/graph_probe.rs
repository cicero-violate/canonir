use anyhow::Result;
use canon_event_store::{
    apply_rustc_event_to_graph, extract_rustc_event, read_any_events_from_path, AnyEvent,
    CodeGraphProjection,
};
use canon_graph::graph::graph_normalize::normalize_graph;
use canon_graph::graph::graph_types::{CodeGraphEdge, CodeGraphNode};
use canon_ir::{CanonIR, CanonId, CanonNodeKind, NodeId};
use canon_analysis::analysis::callgraph::extract_callgraph_edges;
use canon_analysis::analysis::cfg::{build_cfg_in, build_cfg_out, extract_cfg_edges};
use canon_analysis::analysis::runtime_reachability::build_runtime_reachability_report;
use canon_analysis::graph_artifacts::load_latest_workspace_graph_artifact;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use std::process::Command;

#[derive(Serialize)]
struct ProbeReport {
    workspace: String,
    graph_artifact: String,
    source: String,
    symbol_count: usize,
    symbol_sample: Vec<String>,
    call_edge_count: usize,
    cfg_edge_count: usize,
    reachable_functions: usize,
    total_functions: usize,
    coverage_ratio: f64,
    unreachable_sample: Vec<String>,
    cfg_out_sample: Vec<(u32, Vec<u32>)>,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let workspace = arg_value(&args, "--workspace").unwrap_or("/workspace/ai_sandbox/canon".to_string());
    let tlog = arg_value(&args, "--tlog")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(workspace.clone()).join("state/event_log/event.tlog.d"));
    let crate_name = arg_value(&args, "--crate");
    let out_path = arg_value(&args, "--out").map(PathBuf::from);
    let entry_symbol = arg_value(&args, "--entry");
    let symbol_limit = arg_value(&args, "--symbol-limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50);
    let unreachable_limit = arg_value(&args, "--unreachable-limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);
    let cfg_limit = arg_value(&args, "--cfg-limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);

    if let Some(sym) = entry_symbol.as_deref() {
        std::env::set_var("CANON_RUNTIME_ENTRY_SYMBOL", sym);
    }

    let (summary, ir) = if let Some(name) = crate_name.as_deref() {
        match load_crate_graph_artifact(Path::new(&workspace), name) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("[graph_probe] missing graph artifact for {name}: {err}. Running cargo build -p {name}...");
                run_cargo_build(&workspace, name)?;
                load_crate_graph_artifact(Path::new(&workspace), name)?
            }
        }
    } else {
        load_latest_workspace_graph_artifact(Path::new(&workspace))?
    };
    let symbols = graph_symbol_paths(&ir);

    let (source, call_edge_count, cfg_edge_count, reachable_functions, total_functions, coverage_ratio, unreachable_sample, cfg_out_sample) =
        match replay_tlog_graph(&tlog)? {
            Some((nodes, edges, files)) => {
                let callgraph = extract_callgraph_edges(&nodes, &edges);
                let cfg = extract_cfg_edges(&nodes, &edges);
                let cfg_out = build_cfg_out(&cfg);
                let _cfg_in = build_cfg_in(&cfg);
                let node_map: HashMap<u32, CodeGraphNode> = nodes.iter().map(|n| (n.id, n.clone())).collect();
                let file_map: HashMap<u32, String> = files.iter().enumerate().map(|(i, f)| (i as u32, f.clone())).collect();
                let reachability = build_runtime_reachability_report(&node_map, &file_map, &callgraph)?;
                let cfg_out_sample = cfg_out
                    .iter()
                    .take(cfg_limit)
                    .map(|(k, v)| (*k, v.iter().copied().take(10).collect()))
                    .collect::<Vec<_>>();
                let unreachable_sample = reachability
                    .unreachable
                    .iter()
                    .take(unreachable_limit)
                    .map(|u| {
                        if u.file.is_empty() {
                            u.symbol.clone()
                        } else {
                            format!("{} @ {}:{}", u.symbol, u.file, u.line.unwrap_or(0))
                        }
                    })
                    .collect::<Vec<_>>();
                (
                    "tlog".to_string(),
                    callgraph.len(),
                    cfg.len(),
                    reachability.reachable_functions,
                    reachability.total_functions,
                    reachability.coverage_ratio,
                    unreachable_sample,
                    cfg_out_sample,
                )
            }
            None => {
                let (call_edges, fn_nodes) = callgraph_from_canon_ir(&ir);
                let cfg_edges = cfg_from_canon_ir(&ir);
                let cfg_out_sample = cfg_edges
                    .iter()
                    .take(cfg_limit)
                    .map(|(src, dsts)| (*src, dsts.iter().copied().take(10).collect()))
                    .collect::<Vec<_>>();
                let total_functions = fn_nodes.len();
                let (reachable_functions, coverage_ratio, unreachable_sample) = reachability_from_canon_ir(&ir, &fn_nodes, entry_symbol.as_deref());
                (
                    "graph_artifact".to_string(),
                    call_edges.len(),
                    cfg_edges.values().map(|v| v.len()).sum(),
                    reachable_functions,
                    total_functions,
                    coverage_ratio,
                    unreachable_sample.into_iter().take(unreachable_limit).collect(),
                    cfg_out_sample,
                )
            }
        };

    let report = ProbeReport {
        workspace,
        graph_artifact: summary.artifact_path.to_string_lossy().to_string(),
        source,
        symbol_count: symbols.len(),
        symbol_sample: symbols.into_iter().take(symbol_limit).collect(),
        call_edge_count,
        cfg_edge_count,
        reachable_functions,
        total_functions,
        coverage_ratio,
        unreachable_sample,
        cfg_out_sample,
    };

    let payload = serde_json::to_string_pretty(&report)?;
    if let Some(path) = out_path {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, payload.as_bytes())?;
    } else {
        println!("{payload}");
    }

    Ok(())
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].to_string())
}

fn load_crate_graph_artifact(workspace_root: &Path, crate_name: &str) -> Result<(canon_analysis::graph_artifacts::GraphArtifactSummary, CanonIR)> {
    let index_path = workspace_root
        .join("state")
        .join("graph")
        .join("index")
        .join("by_crate")
        .join(format!("{crate_name}.json"));
    let index = serde_json::from_slice::<canon_analysis::graph_artifacts::GraphArtifactSummary>(&fs::read(index_path)?)?;
    let ir = canon_analysis::graph_artifacts::load_graph_artifact(&index.artifact_path)?;
    Ok((index, ir))
}

fn run_cargo_build(workspace_root: &str, crate_name: &str) -> Result<()> {
    let status = Command::new("cargo")
        .current_dir(workspace_root)
        .arg("build")
        .arg("-p")
        .arg(crate_name)
        .status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("cargo build -p {crate_name} failed with status: {status}"));
    }
    Ok(())
}

fn graph_symbol_paths(ir: &CanonIR) -> Vec<String> {
    let module_map = module_membership_map(ir);
    let mut out = Vec::new();
    for node in &ir.nodes {
        let Some((name, _kind)) = symbol_identity(ir, &node.kind) else {
            continue;
        };
        let module_path = module_map.get(&node.id.0).map(String::as_str);
        out.push(qualify_symbol_path(module_path, &name));
    }
    out.sort();
    out.dedup();
    out
}

fn module_membership_map(ir: &CanonIR) -> HashMap<u32, String> {
    let mut membership = HashMap::new();
    for node in &ir.nodes {
        let CanonNodeKind::Module { path_id, .. } = &node.kind else {
            continue;
        };
        let module_path = ir.lookup_path(*path_id).to_string();
        for (dst, _) in ir.module_graph.neighbours(NodeId(node.id.0)) {
            membership.entry(dst.0).or_insert_with(|| module_path.clone());
        }
    }
    membership
}

fn symbol_identity(ir: &CanonIR, kind: &CanonNodeKind) -> Option<(String, &'static str)> {
    match kind {
        CanonNodeKind::Struct { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "struct")),
        CanonNodeKind::Enum { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "enum")),
        CanonNodeKind::Trait { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "trait")),
        CanonNodeKind::AssocType { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "assoc_type")),
        CanonNodeKind::AssocConst { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "assoc_const")),
        CanonNodeKind::Fn { name_id, .. } => Some((ir.lookup_name(*name_id).to_string(), "fn")),
        _ => None,
    }
}

fn qualify_symbol_path(module_path: Option<&str>, name: &str) -> String {
    match module_path {
        Some(module_path) if !module_path.is_empty() => format!("{module_path}::{name}"),
        _ => format!("crate::{name}"),
    }
}

fn replay_tlog_graph(tlog_path: &Path) -> Result<Option<(Vec<CodeGraphNode>, Vec<CodeGraphEdge>, Vec<String>)>> {
    let mut graph = CodeGraphProjection::default();
    let mut symbol_to_id: HashMap<String, u32> = HashMap::new();
    let events = read_any_events_from_path(tlog_path)?;
    for event in events {
        let AnyEvent::Canon(canon) = event else {
            continue;
        };
        let Some(kernel) = extract_rustc_event(&canon) else {
            continue;
        };
        apply_rustc_event_to_graph(kernel, &mut graph, &mut symbol_to_id, false);
    }
    let (nodes, edges, files) = normalize_graph(graph.nodes, graph.edges, graph.files);
    if nodes.is_empty() && edges.is_empty() {
        return Ok(None);
    }
    Ok(Some((nodes, edges, files)))
}

fn callgraph_from_canon_ir(ir: &CanonIR) -> (Vec<(u32, u32)>, Vec<u32>) {
    let mut fn_nodes = Vec::new();
    for node in &ir.nodes {
        if matches!(node.kind, CanonNodeKind::Fn { .. }) {
            fn_nodes.push(node.id.0);
        }
    }
    let mut edges = Vec::new();
    for (row, src_id) in ir.call_graph.node_data.iter().enumerate() {
        let src_id = src_id.0;
        if !matches!(ir.node(CanonId(src_id)).kind, CanonNodeKind::Fn { .. }) {
            continue;
        }
        let start = ir.call_graph.row_ptr[row] as usize;
        let end = ir.call_graph.row_ptr[row + 1] as usize;
        for idx in start..end {
            let dst_row = ir.call_graph.col_idx[idx] as usize;
            let dst_id = ir.call_graph.node_data[dst_row].0;
            if matches!(ir.node(CanonId(dst_id)).kind, CanonNodeKind::Fn { .. }) {
                edges.push((src_id, dst_id));
            }
        }
    }
    (edges, fn_nodes)
}

fn cfg_from_canon_ir(ir: &CanonIR) -> HashMap<u32, Vec<u32>> {
    let mut out: HashMap<u32, Vec<u32>> = HashMap::new();
    for (row, src_id) in ir.cfg_graph.node_data.iter().enumerate() {
        let src_id = src_id.0;
        let start = ir.cfg_graph.row_ptr[row] as usize;
        let end = ir.cfg_graph.row_ptr[row + 1] as usize;
        if start == end {
            continue;
        }
        let mut dsts = Vec::new();
        for idx in start..end {
            let dst_row = ir.cfg_graph.col_idx[idx] as usize;
            let dst_id = ir.cfg_graph.node_data[dst_row].0;
            dsts.push(dst_id);
        }
        out.insert(src_id, dsts);
    }
    out
}

fn reachability_from_canon_ir(ir: &CanonIR, fn_nodes: &[u32], entry: Option<&str>) -> (usize, f64, Vec<String>) {
    let entry = entry.unwrap_or("main");
    let mut entry_id: Option<u32> = None;
    for node in &ir.nodes {
        let CanonNodeKind::Fn { name_id, .. } = &node.kind else { continue };
        if ir.lookup_name(*name_id) == entry {
            entry_id = Some(node.id.0);
            break;
        }
    }
    let Some(entry_id) = entry_id else {
        return (0, 0.0, Vec::new());
    };

    let (edges, _) = callgraph_from_canon_ir(ir);
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for (s, d) in edges {
        adj.entry(s).or_default().push(d);
    }

    let mut reachable = std::collections::HashSet::new();
    let mut stack = vec![entry_id];
    while let Some(id) = stack.pop() {
        if !reachable.insert(id) {
            continue;
        }
        if let Some(next) = adj.get(&id) {
            stack.extend(next.iter().copied());
        }
    }

    let total = fn_nodes.len();
    let reachable_count = reachable.len();
    let ratio = if total == 0 { 0.0 } else { reachable_count as f64 / total as f64 };
    let mut unreachable = Vec::new();
    for id in fn_nodes {
        if reachable.contains(id) {
            continue;
        }
        let node = ir.node(CanonId(*id));
        if let CanonNodeKind::Fn { name_id, .. } = &node.kind {
            unreachable.push(ir.lookup_name(*name_id).to_string());
        }
    }
    (reachable_count, ratio, unreachable)
}

use anyhow::{anyhow, Result};
use serde_json::Value;
use csv::Writer;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use rayon::prelude::*;
use algorithms::graph::csr::Csr;
#[cfg(feature = "cuda")]
use algorithms::graph::scc_gpu::scc_gpu;
#[cfg(feature = "cuda")]
use algorithms::graph::reachability::{reachability_gpu, reachability_batched_gpu};
use crate::artifacts_loader::{KernelGraph as LoadedGraph, Node as GraphNode, Edge as GraphEdge, CsrGraph};
use crate::artifacts::snapshot::{SnapshotMeta, save_graph_snapshot, write_snapshot_metadata};
use crate::artifacts::cache::{update_graph_cache};
use crate::kernel_invariants::write_kernel_invariants;
use crate::graph::graph_types::{EdgeRow, ModuleNode, NodeRow};
use crate::graph::graph_builder::rows_to_kernel_graph;
use crate::graph::graph_normalize::normalize_graph;
use crate::replay::tlog_replay::{parse_tlog_event, replay_graph_from_tlog_incremental};
use crate::replay::session_scan::{find_last_graph_session_offset, find_last_session_offset, session_contains_module_nodes};
use std::io::BufReader;

#[derive(Debug, Serialize, Default)]
struct CallsiteResolutionReport {
    total_callsites: u64,
    resolved: u64,
    unresolved: u64,
    by_type: BTreeMap<String, CallsiteResolutionCounts>,
}

#[derive(Debug, Serialize, Default)]
struct CallsiteResolutionCounts {
    total: u64,
    resolved: u64,
    unresolved: u64,
}


#[derive(Serialize)]
struct BranchComplexityEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    branch_count: usize,
    duplicate_block_count: usize,
    score: usize,
}

#[derive(Serialize)]
struct CallgraphCentralityEntry {
    symbol: String,
    file: String,
    caller_count: usize,
    callee_count: usize,
    centrality_score: usize,
}

#[derive(Serialize)]
struct DeadCodeEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    reason: String,
}

#[derive(Serialize)]
struct DependencyCycleEntry {
    cycle_id: usize,
    nodes: Vec<String>,
    files: Vec<String>,
    cycle_length: usize,
}

#[derive(Serialize)]
struct StructuralHotspotEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    branch_count: usize,
    duplicate_blocks: usize,
    callers: Vec<String>,
    score: usize,
}

#[derive(Serialize)]
struct DataflowFanoutEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    outgoing_edges: usize,
    mutation_edges: usize,
    io_edges: usize,
}

#[derive(Serialize)]
struct BranchPressureEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    branch_nodes: usize,
    branch_pressure: usize,
}

#[derive(Serialize)]
struct MergeCandidateEntry {
    function: String,
    branch_block: u32,
    successors: Vec<u32>,
    candidate_blocks: Vec<u32>,
}

#[derive(Serialize)]
struct ReachabilityEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    reachable_blocks: usize,
    total_blocks: usize,
    reachable_ratio: f64,
}

#[derive(Serialize)]
struct PathRedundancyEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    paths_total: usize,
    paths_unique: usize,
    redundancy_ratio: f64,
}

pub fn generate_reports(output_dir: &Path, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)?;
    let nodes = read_nodes_csv(output_dir.join("nodes.csv"))?;
    let edges = read_edges_csv(output_dir.join("edges.csv"))?;
    let files = read_files_txt(output_dir.join("files.txt"))?;
    let _symbols_json = fs::read_to_string(output_dir.join("symbols.json"))
        .map_err(|e| anyhow!("failed to read symbols.json: {e}"))?;
    let reports_dir = out_dir.join("reports");
    generate_reports_from_parts(nodes, edges, files, out_dir, &reports_dir)
}

pub fn generate_reports_from_tlog(tlog_path: &Path, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)?;
    let snapshot_path = out_dir.join("graph_snapshot.bin");
    let meta_path = out_dir.join("snapshot.meta.json");
    let graph_bin_path = out_dir.join("graph.bin");
    let skip_snapshot = std::env::var("CANON_REPORTS_SKIP_SNAPSHOT").ok().as_deref() == Some("1");
    let graph_bin_fresh = graph_bin_path.exists() && is_graph_bin_fresh(&graph_bin_path, tlog_path);
    let tlog_has_modules = if graph_bin_fresh
        || (std::env::var("CANON_REPORTS_MINIMAL").ok().as_deref() == Some("1")
            && graph_bin_path.exists())
    {
        true
    } else {
        session_contains_module_nodes(tlog_path)
    };
    let prefer_graph_bin = graph_bin_path.exists() && !graph_bin_fresh && !tlog_has_modules;
    let mut force_write_graph_bin = false;
    let (mut nodes, mut edges, mut files) = if graph_bin_fresh || prefer_graph_bin {
        load_graph_bin(&graph_bin_path)?
    } else {
        replay_graph_from_tlog_incremental(tlog_path, &snapshot_path, &meta_path)?
    };
    if nodes.is_empty() && edges.is_empty() {
        if find_last_graph_session_offset(tlog_path).is_some() {
            let refreshed = replay_graph_from_tlog_incremental(tlog_path, &snapshot_path, &meta_path)?;
            nodes = refreshed.0;
            edges = refreshed.1;
            files = refreshed.2;
            force_write_graph_bin = true;
        }
    }
    let (nodes, edges, files) = normalize_graph(nodes, edges, files);
    if (!graph_bin_fresh && !prefer_graph_bin) || force_write_graph_bin {
        write_graph_bin(&graph_bin_path, &nodes, &edges, &files)?;
        if skip_snapshot {
            eprintln!(
                "canon_reports: skipping kernel snapshot write (CANON_REPORTS_SKIP_SNAPSHOT=1)"
            );
        } else {
            let snapshot_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                save_graph_snapshot(&snapshot_path, &nodes, &edges, &files)
            }));
            match snapshot_result {
                Ok(res) => res?,
                Err(_) => {
                    eprintln!(
                        "canon_reports: kernel snapshot write panicked (rkyv ExceedsStorageRange likely). Continuing without snapshot."
                    );
                }
            }
        }
        let meta = SnapshotMeta {
            tlog_offset: tlog_path.metadata().map(|m| m.len()).unwrap_or(0),
            event_count: (nodes.len() + edges.len()) as u64,
            created_at: current_timestamp(),
            version: 2,
        };
        write_snapshot_metadata(&meta_path, &meta)?;
    }
    let reports_dir = out_dir.join("reports");
    generate_reports_from_parts(nodes, edges, files, out_dir, &reports_dir)?;
    write_tlog_integrity_report(tlog_path, &reports_dir)?;
    write_system_health_report(tlog_path, &reports_dir)?;
    if let Err(err) = write_callsite_resolution_from_tlog(tlog_path, &reports_dir) {
        eprintln!("canon_reports: callsite resolution failed: {err:?}");
    }
    if let Ok(cache) = load_and_update_graph_cache(tlog_path, &reports_dir) {
        let (modulegraph, module_nodes) = build_modulegraph_from_cache(&cache);
        write_modulegraph_csv(out_dir, &modulegraph, &module_nodes)?;
        if !cache.type_nodes.is_empty() || !cache.type_edges.is_empty() {
            let (typegraph, type_nodes) = build_typegraph_from_cache(&cache);
            write_typegraph_csv_from_cache(out_dir, &typegraph, &type_nodes)?;
        }
    }
    if let Err(err) = crate::invariant_validator::run_invariant_pipeline(out_dir) {
        eprintln!("canon_reports: invariant pipeline failed: {err:?}");
    }
    Ok(())
}

fn generate_reports_from_parts(
    nodes: Vec<NodeRow>,
    edges: Vec<EdgeRow>,
    files: Vec<String>,
    graph_dir: &Path,
    reports_dir: &Path,
) -> Result<()> {
    fs::create_dir_all(graph_dir)?;
    fs::create_dir_all(reports_dir)?;
    let (nodes, edges, files) = normalize_graph(nodes, edges, files);
    let (cfg, callgraph) = write_graph_artifacts(graph_dir, &nodes, &edges, &files)?;
    let kernel_graph = rows_to_kernel_graph(&nodes, &edges, &files);
    if let Err(err) = write_kernel_invariants(graph_dir, reports_dir, &kernel_graph) {
        eprintln!("[reports] kernel invariants failed: {err}");
    }

    if std::env::var("CANON_REPORTS_MINIMAL").ok().as_deref() == Some("1") {
        write_graph_health_report(graph_dir, reports_dir, &nodes, &edges, &files, &cfg, &callgraph)?;
        return Ok(());
    }

    let node_map: HashMap<u32, NodeRow> = nodes.iter().map(|n| (n.id, n.clone())).collect();

    let mut file_map: HashMap<u32, String> = HashMap::new();
    for (idx, path) in files.iter().enumerate() {
        file_map.insert(idx as u32, path.clone());
    }

    let cfg_out = build_cfg_out(&cfg);
    let cfg_in = build_cfg_in(&cfg);

    let block_owner = build_block_owner(&nodes, &edges);
    let block_effect_sig = build_block_effect_signatures(&edges, &node_map);

    // Build callgraph CSR once — shared by GPU SCC, GPU reachability, dead code
    let (cg_csr, cg_id_to_local, cg_local_to_id) = build_callgraph_csr(&callgraph);

    // Parallel dispatch — independent reports run concurrently
    let results: Vec<Result<()>> = [
        "branch_complexity",
        "callgraph_centrality",
        "dead_code",
        "dependency_cycles",
        "structural_hotspots",
        "dataflow_fanout",
        "branch_pressure",
        "merge_candidates",
        "reachability",
        "path_redundancy",
    ]
    .into_par_iter()
    .map(|report| match report {
        "branch_complexity" => {
            let r = build_branch_complexity(
                &nodes,
                &node_map,
                &file_map,
                &cfg_out,
                &cfg_in,
                &block_effect_sig,
            );
            write_report(&reports_dir.join("branch_complexity_report.json"), &r)
        }
        "callgraph_centrality" => {
            let r = build_callgraph_centrality(&callgraph, &node_map, &file_map);
            write_report(&reports_dir.join("callgraph_centrality_report.json"), &r)
        }
        "dead_code" => {
            let r = build_dead_code_gpu(
                &nodes,
                &node_map,
                &file_map,
                &edges,
                &cfg_out,
                &cfg_in,
                &callgraph,
                &block_owner,
                &cg_csr,
                &cg_id_to_local,
                &cg_local_to_id,
            );
            write_report(&reports_dir.join("dead_code_report.json"), &r)
        }
        "dependency_cycles" => {
            let r = build_dependency_cycles_gpu(
                &callgraph,
                &node_map,
                &file_map,
                &cg_csr,
                &cg_local_to_id,
            );
            write_report(&reports_dir.join("dependency_cycle_report.json"), &r)
        }
        "structural_hotspots" => {
            let r = build_structural_hotspots(
                &nodes,
                &node_map,
                &file_map,
                &callgraph,
                &cfg_out,
                &cfg_in,
                &block_owner,
                &block_effect_sig,
            );
            write_report(&reports_dir.join("structural_hotspots_report.json"), &r)
        }
        "dataflow_fanout" => {
            let r = build_dataflow_fanout(&nodes, &node_map, &file_map, &edges, &block_owner);
            write_report(&reports_dir.join("dataflow_fanout_report.json"), &r)
        }
        "branch_pressure" => {
            let r = build_branch_pressure(&block_owner, &node_map, &file_map, &cfg_out);
            write_report(&reports_dir.join("branch_pressure_report.json"), &r)
        }
        "merge_candidates" => {
            let r = build_merge_candidates(&cfg_out, &block_owner, &node_map, &file_map);
            write_report(&reports_dir.join("merge_candidates_report.json"), &r)
        }
        "reachability" => {
            let r = build_reachability_report_gpu(
                &cfg_out,
                &block_owner,
                &node_map,
                &file_map,
                &cg_csr,
                &cg_id_to_local,
                &cg_local_to_id,
            );
            write_report(&reports_dir.join("reachability_report.json"), &r)
        }
        "path_redundancy" => {
            let r = build_path_redundancy(&cfg_out, &block_owner, &node_map, &file_map);
            write_report(&reports_dir.join("path_redundancy_report.json"), &r)
        }
        _ => Ok(()),
    })
    .collect();

    for result in results {
        result?;
    }

    write_graph_health_report(graph_dir, reports_dir, &nodes, &edges, &files, &cfg, &callgraph)?;
    write_semantic_signatures(graph_dir, reports_dir, &nodes, &edges, &callgraph)?;
    write_semantic_clusters(graph_dir, reports_dir, &nodes, &edges, &callgraph)?;

    Ok(())
}

fn write_callsite_resolution_from_tlog(tlog_path: &Path, reports_dir: &Path) -> Result<()> {
    let offset = find_last_graph_session_offset(tlog_path)
        .or_else(|| find_last_session_offset(tlog_path))
        .unwrap_or(0);
    let file = fs::File::open(tlog_path)?;
    let mut reader = BufReader::new(file);
    if offset > 0 {
        reader.seek(SeekFrom::Start(offset))?;
    }
    let mut report = CallsiteResolutionReport::default();
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        let Some(record) = parse_tlog_event(&line) else {
            continue;
        };
        if record.get("t").and_then(|v| v.as_str()) != Some("CALLSITE") {
            continue;
        }
        let kind = record.get("kind").and_then(|v| v.as_str()).unwrap_or("other");
        let resolved = record.get("resolved").and_then(|v| v.as_bool()).unwrap_or(false);
        report.total_callsites += 1;
        if resolved {
            report.resolved += 1;
        } else {
            report.unresolved += 1;
        }
        let entry = report.by_type.entry(kind.to_string()).or_default();
        entry.total += 1;
        if resolved {
            entry.resolved += 1;
        } else {
            entry.unresolved += 1;
        }
    }
    fs::create_dir_all(reports_dir)?;
    fs::write(
        reports_dir.join("callsite_resolution.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    Ok(())
}

#[allow(dead_code)]
fn write_graph_artifacts(
    out_dir: &Path,
    nodes: &[NodeRow],
    edges: &[EdgeRow],
    files: &[String],
) -> Result<(Vec<EdgeRow>, Vec<(u32, u32)>)> {
    let mut cfg = build_cfg_edges(nodes, edges);
    cfg.sort_by(|a, b| {
        a.src
            .cmp(&b.src)
            .then_with(|| a.dst.cmp(&b.dst))
            .then_with(|| a.kind.cmp(&b.kind))
    });

    let mut callgraph = build_callgraph_edges(nodes, edges);
    callgraph.sort_unstable();

    let (modulegraph, module_nodes) = build_modulegraph(nodes, files);
    let mut modulegraph = modulegraph;
    modulegraph.sort_unstable();

    let mut typegraph = build_typegraph_edges(nodes, edges);
    typegraph.sort_by(|a, b| {
        a.0
            .cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });

    write_cfg_csv(out_dir, &cfg)?;
    write_callgraph_csv(out_dir, &callgraph, nodes, files)?;
    write_modulegraph_csv(out_dir, &modulegraph, &module_nodes)?;
    write_typegraph_csv(out_dir, &typegraph, nodes, files)?;

    Ok((cfg, callgraph))
}

fn build_cfg_edges(nodes: &[NodeRow], edges: &[EdgeRow]) -> Vec<EdgeRow> {
    let id_to_kind: HashMap<u32, &str> = nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut out = Vec::new();
    for edge in edges {
        if edge.kind != "FLOW" && edge.kind != "UNWIND" && edge.kind != "RETURN" && edge.kind != "BRANCH" {
            continue;
        }
        let src_kind = id_to_kind.get(&edge.src);
        let dst_kind = id_to_kind.get(&edge.dst);
        if src_kind == Some(&"BASIC_BLOCK") {
            if edge.kind == "RETURN" {
                out.push(edge.clone());
                continue;
            }
            if dst_kind == Some(&"BASIC_BLOCK") {
                out.push(edge.clone());
            }
        }
    }
    out
}

fn build_callgraph_edges(nodes: &[NodeRow], edges: &[EdgeRow]) -> Vec<(u32, u32)> {
    let id_to_kind: HashMap<u32, &str> = nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut seen: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut callsite_to_block: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut block_to_fn: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut has_callsite_edges = false;

    for edge in edges {
        if edge.kind != "HAS_BLOCK" {
            continue;
        }
        let src_kind = id_to_kind.get(&edge.src);
        let dst_kind = id_to_kind.get(&edge.dst);
        if src_kind == Some(&"BASIC_BLOCK") && dst_kind == Some(&"CALL_SITE") {
            callsite_to_block.entry(edge.dst).or_default().insert(edge.src);
            has_callsite_edges = true;
        } else if matches!(src_kind, Some(&"FUNCTION" | &"METHOD")) && dst_kind == Some(&"BASIC_BLOCK") {
            block_to_fn.entry(edge.dst).or_default().insert(edge.src);
        }
    }

    for edge in edges {
        if edge.kind != "CALL" {
            continue;
        }
        let callee_kind = id_to_kind.get(&edge.dst);
        if !matches!(callee_kind, Some(&"FUNCTION" | &"METHOD")) {
            continue;
        }
        if has_callsite_edges {
            if let Some(blocks) = callsite_to_block.get(&edge.src) {
                for block in blocks {
                    if let Some(callers) = block_to_fn.get(block) {
                        for caller in callers {
                            seen.insert((*caller, edge.dst));
                        }
                    }
                }
            }
        } else {
            let caller_kind = id_to_kind.get(&edge.src);
            if matches!(caller_kind, Some(&"FUNCTION" | &"METHOD")) {
                seen.insert((edge.src, edge.dst));
            }
        }
    }

    seen.into_iter().collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]

// DEPRECATED: Batch report generation pipeline\n// This module generates reports by scanning the entire tlog.\n// The runtime system now uses ReportEventConsumer for incremental updates.\n// This module is retained only for offline rebuilds and debugging.\n
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use rayon::prelude::*;
use canon_event_store::{save_graph_snapshot, write_snapshot_metadata, SnapshotMeta};
use canon_graph::artifacts::cache::{update_graph_cache};
use canon_graph::artifacts::artifact_writer::{
    is_graph_bin_fresh,
    load_graph_bin,
    emit_graph_bin,
    emit_cfg_csv,
    emit_cfg_full_csv,
    emit_callgraph_csv,
    emit_callgraph_full_csv,
    emit_modulegraph_csv,
    emit_nodes_csv,
    emit_nodes_full_csv,
    emit_nodes_raw_jsonl,
    emit_edges_csv,
    emit_edges_full_csv,
    emit_files_txt,
    emit_typegraph_csv,
    emit_typegraph_full_csv,
    emit_typegraph_csv_from_cache,
    build_modulegraph,
    build_modulegraph_from_cache,
    build_typegraph_edges,
    build_typegraph_from_cache,
};
use crate::invariants::kernel_invariants::write_kernel_invariants;
use canon_graph::graph::graph_types::{CodeGraphEdge, CodeGraphNode};
use canon_graph::graph::graph_builder::rows_to_code_graph;
use canon_graph::graph::csr::build_callgraph_csr_graph;
use canon_graph::graph::graph_normalize::normalize_graph;
use crate::analysis::cfg::{extract_cfg_edges, build_cfg_out, build_cfg_in, build_block_owner, build_block_effect_signatures};
use crate::analysis::callgraph::{extract_callgraph_edges, build_callgraph_centrality};
use crate::analysis::dead_code::{detect_dead_code_gpu};
use crate::analysis::dependency_cycles::build_dependency_cycles_gpu;
use crate::analysis::structural_hotspots::{build_structural_hotspots, build_branch_complexity, build_branch_pressure, build_merge_candidates, build_path_redundancy, build_reachability_report_gpu};
use crate::analysis::dataflow::build_dataflow_fanout;
use crate::analysis::panic_report::build_panic_report;
use canon_graph::health::graph_health::write_graph_health_report;
use canon_graph::health::tlog_integrity::write_tlog_integrity_report;
use canon_graph::health::system_health::{write_system_health_report, current_timestamp};
use crate::semantics::semantic_features::extract_node_features;
use crate::semantics::semantic_signature::compute_signatures;
use crate::semantics::semantic_clustering::cluster_dbscan_like;
use canon_types::{RustcEvent, ReportLayout};
use canon_event_store::{
    extract_rustc_event, find_last_graph_session_offset, read_any_events_from_path,
    replay_graph_from_tlog, replay_graph_from_tlog_incremental, session_contains_module_nodes,
};

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


pub fn generate_reports(output_dir: &Path, out_dir: &Path) -> Result<()> {
    let layout = ReportLayout::from_crate_root(out_dir.to_path_buf());
    layout.ensure_dirs()?;
    let graph_dir = layout.graph_dir();
    let nodes = read_nodes_csv(output_dir.join("nodes.csv"))?;
    let edges = read_edges_csv(output_dir.join("edges.csv"))?;
    let files = read_files_txt(output_dir.join("files.txt"))?;
    let _symbols_json = fs::read_to_string(output_dir.join("symbols.json"))
        .map_err(|e| anyhow!("failed to read symbols.json: {e}"))?;
    let _ = generate_reports_from_parts(nodes, edges, files, &layout, &graph_dir)?;
    Ok(())
}

pub fn generate_reports_from_tlog(tlog_path: &Path, out_dir: &Path) -> Result<()> {
    let layout = ReportLayout::from_crate_root(out_dir.to_path_buf());
    layout.ensure_dirs()?;
    let graph_dir = layout.graph_dir();
    let graphs_dir = layout.graphs_dir();
    let analysis_dir = layout.analysis_dir();
    let metrics_dir = layout.metrics_dir();
    let invariants_dir = layout.invariants_dir();
    let meta_dir = layout.meta_dir();

    let snapshot_path = graph_dir.join("graph_snapshot.bin");
    let meta_path = graph_dir.join("snapshot.meta.json");
    let graph_bin_path = graph_dir.join("graph.bin");
    let minimal = std::env::var("CANON_REPORTS_MINIMAL").ok().as_deref() == Some("1");
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
        let replay = replay_graph_from_tlog_incremental(tlog_path, &snapshot_path, &meta_path)?;
        (replay.nodes, replay.edges, replay.files)
    };
    if nodes.is_empty() && edges.is_empty() {
        if find_last_graph_session_offset(tlog_path).is_some() {
            let refreshed = replay_graph_from_tlog_incremental(tlog_path, &snapshot_path, &meta_path)?;
            nodes = refreshed.nodes;
            edges = refreshed.edges;
            files = refreshed.files;
            force_write_graph_bin = true;
        }
    }
    let (mut nodes, mut edges, mut files) = normalize_graph(nodes, edges, files);
    if std::env::var("CANON_REPORTS_VERIFY_DETERMINISM").ok().as_deref() == Some("1") {
        let replay = replay_graph_from_tlog(tlog_path)?;
        let (r_nodes, r_edges, r_files) = normalize_graph(replay.nodes, replay.edges, replay.files);
        let left = graph_fingerprint(&nodes, &edges, &files);
        let right = graph_fingerprint(&r_nodes, &r_edges, &r_files);
        if left != right {
            if std::env::var("CANON_REPORTS_VERIFY_DETERMINISM_STRICT")
                .ok()
                .as_deref()
                == Some("1")
            {
                return Err(anyhow!(
                    "determinism check failed: graph fingerprint mismatch (left={}, right={})",
                    left,
                    right
                ));
            }
            eprintln!(
                "canon_reports: determinism mismatch (left={}, right={}); falling back to full replay",
                left, right
            );
            nodes = r_nodes;
            edges = r_edges;
            files = r_files;
            force_write_graph_bin = true;
        }
    }
    if (!graph_bin_fresh && !prefer_graph_bin) || force_write_graph_bin {
        emit_graph_bin(&graph_bin_path, &nodes, &edges, &files)?;
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
    let parts = generate_reports_from_parts(nodes, edges, files, &layout, &graph_dir)?;
    if let Ok(cache) = update_graph_cache(tlog_path, &graph_dir) {
        let (modulegraph, module_nodes) = build_modulegraph_from_cache(&cache);
        emit_modulegraph_csv(&graphs_dir, &modulegraph, &module_nodes)?;
        if !cache.type_nodes.is_empty() || !cache.type_edges.is_empty() {
            let (typegraph, type_nodes) = build_typegraph_from_cache(&cache);
            emit_typegraph_csv_from_cache(&graphs_dir, &typegraph, &type_nodes)?;
        }
    }
    write_symbol_artifacts_from_tlog(tlog_path, &graph_dir)?;
    if let Err(err) = write_callsite_resolution_from_tlog(tlog_path, &analysis_dir) {
        eprintln!("canon_reports: callsite resolution failed: {err:?}");
    }
    if !minimal {
        if let Err(err) = crate::invariants::invariant_validator::run_invariant_pipeline(
            &graph_dir,
            &invariants_dir,
            &meta_dir,
            &analysis_dir,
            &metrics_dir,
        ) {
            eprintln!("canon_reports: invariant pipeline failed: {err:?}");
        }
    }
    write_graph_health_report(
        &graph_dir,
        &metrics_dir,
        &parts.nodes,
        &parts.edges,
        &parts.files,
        &parts.cfg,
        &parts.callgraph,
    )?;
    write_tlog_integrity_report(tlog_path, &metrics_dir)?;
    write_system_health_report(tlog_path, &metrics_dir)?;
    let panic_log = analysis_dir.join("panic_records.jsonl");
    let panic_summary = analysis_dir.join("panic_summary.json");
    if let Err(err) = build_panic_report(&panic_log, &panic_summary) {
        eprintln!("canon_reports: panic report failed: {err:?}");
    }
    cleanup_legacy_dirs(layout.root())?;
    Ok(())
}

struct ReportParts {
    nodes: Vec<CodeGraphNode>,
    edges: Vec<CodeGraphEdge>,
    files: Vec<String>,
    cfg: Vec<CodeGraphEdge>,
    callgraph: Vec<(u32, u32)>,
}

fn generate_reports_from_parts(
    nodes: Vec<CodeGraphNode>,
    edges: Vec<CodeGraphEdge>,
    files: Vec<String>,
    layout: &ReportLayout,
    graph_dir: &Path,
) -> Result<ReportParts> {
    let graphs_dir = layout.graphs_dir();
    let analysis_dir = layout.analysis_dir();
    let metrics_dir = layout.metrics_dir();
    fs::create_dir_all(graph_dir)?;
    fs::create_dir_all(&graphs_dir)?;
    fs::create_dir_all(&analysis_dir)?;
    fs::create_dir_all(&metrics_dir)?;
    let (nodes, edges, files) = normalize_graph(nodes, edges, files);
    let (cfg, callgraph) = write_graph_artifacts(graph_dir, &graphs_dir, &nodes, &edges, &files)?;
    let kernel_graph = rows_to_code_graph(&nodes, &edges, &files);
    if let Err(err) = write_kernel_invariants(graph_dir, &metrics_dir, &kernel_graph) {
        eprintln!("[reports] kernel invariants failed: {err}");
    }

    if std::env::var("CANON_REPORTS_MINIMAL").ok().as_deref() == Some("1") {
        return Ok(ReportParts { nodes, edges, files, cfg, callgraph });
    }

    let node_map: HashMap<u32, CodeGraphNode> = nodes.iter().map(|n| (n.id, n.clone())).collect();

    let diagnostics = build_diagnostics(&nodes, &edges);
    write_diagnostics(&analysis_dir, &diagnostics)?;
    if diagnostics.should_fail {
        write_missing_report_placeholders(&analysis_dir, &metrics_dir, &diagnostics)?;
        return Err(anyhow!(diagnostics.fail_reason));
    }

    let mut file_map: HashMap<u32, String> = HashMap::new();
    for (idx, path) in files.iter().enumerate() {
        file_map.insert(idx as u32, path.clone());
    }

    let cfg_out = build_cfg_out(&cfg);
    let cfg_in = build_cfg_in(&cfg);
    let diagnostics = enrich_diagnostics_with_topology(diagnostics, &cfg_out);
    write_diagnostics(&analysis_dir, &diagnostics)?;
    if std::env::var("CANON_REPORTS_PANIC_ON_EMPTY_CFG").ok().as_deref() == Some("1") && cfg_out.is_empty() {
        panic!("CFG invariant violated: no CFG edges found");
    }
    if std::env::var("CANON_REPORTS_PANIC_ON_EMPTY_CALLGRAPH").ok().as_deref() == Some("1") && callgraph.is_empty() {
        panic!("Callgraph invariant violated: no call edges");
    }
    if std::env::var("CANON_REPORTS_PANIC_ON_CALLSITE_MISMATCH").ok().as_deref() == Some("1")
        && diagnostics.call_edges > 0
        && diagnostics.callsite_nodes == 0
    {
        panic!("Callsite invariant violated: CALL edges present but CALL_SITE nodes missing");
    }
    if std::env::var("CANON_REPORTS_PANIC_ON_BLOCK_MISMATCH").ok().as_deref() == Some("1")
        && diagnostics.function_nodes > 0
        && (diagnostics.has_block_edges == 0 || diagnostics.flow_edges == 0)
    {
        panic!("CFG invariant violated: functions exist but HAS_BLOCK/FLOW edges missing");
    }
    if std::env::var("CANON_REPORTS_PANIC_ON_NO_BRANCHES").ok().as_deref() == Some("1")
        && diagnostics.function_nodes > 0
        && diagnostics.branch_nodes == 0
    {
        panic!("CFG invariant violated: no branch nodes (fan-out > 1) detected");
    }
    if std::env::var("CANON_REPORTS_PANIC_ON_SPARSE_CALLGRAPH").ok().as_deref() == Some("1")
        && diagnostics.function_nodes > 0
        && diagnostics.calls_per_function < 0.05
    {
        panic!("Callgraph invariant violated: calls_per_function < 0.05");
    }

    let block_owner = build_block_owner(&nodes, &edges);
    let block_effect_sig = build_block_effect_signatures(&edges, &node_map);

    // Build callgraph CSR once — shared by GPU SCC, GPU reachability, dead code
    let (cg_csr, cg_id_to_local, cg_local_to_id) = build_callgraph_csr_graph(&callgraph);

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
            write_report(&metrics_dir.join("branch_complexity_report.json"), &r)
        }
        "callgraph_centrality" => {
            let r = build_callgraph_centrality(&callgraph, &node_map, &file_map);
            write_report(&metrics_dir.join("callgraph_centrality_report.json"), &r)
        }
        "dead_code" => {
            let r = detect_dead_code_gpu(
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
            write_report(&analysis_dir.join("dead_code_report.json"), &r)?;
            write_report(&analysis_dir.join("dead_code.json"), &r)
        }
        "dependency_cycles" => {
            let r = build_dependency_cycles_gpu(
                &callgraph,
                &node_map,
                &file_map,
                &cg_csr,
                &cg_local_to_id,
            );
            write_report(&analysis_dir.join("dependency_cycle_report.json"), &r)?;
            write_report(&analysis_dir.join("cycles.json"), &r)
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
            write_report(&metrics_dir.join("structural_hotspots_report.json"), &r)?;
            write_report(&analysis_dir.join("hotspots.json"), &r)
        }
        "dataflow_fanout" => {
            let r = build_dataflow_fanout(&nodes, &node_map, &file_map, &edges, &block_owner);
            write_report(&metrics_dir.join("dataflow_fanout_report.json"), &r)
        }
        "branch_pressure" => {
            let r = build_branch_pressure(&block_owner, &node_map, &file_map, &cfg_out);
            write_report(&metrics_dir.join("branch_pressure_report.json"), &r)
        }
        "merge_candidates" => {
            let r = build_merge_candidates(&cfg_out, &block_owner, &node_map, &file_map);
            write_report(&metrics_dir.join("merge_candidates_report.json"), &r)
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
            write_report(&metrics_dir.join("reachability_report.json"), &r)
        }
        "path_redundancy" => {
            let r = build_path_redundancy(&cfg_out, &block_owner, &node_map, &file_map);
            write_report(&metrics_dir.join("path_redundancy_report.json"), &r)
        }
        _ => Ok(()),
    })
    .collect();

    for result in results {
        result?;
    }

    write_graph_health_report(graph_dir, &metrics_dir, &nodes, &edges, &files, &cfg, &callgraph)?;
    write_semantic_signatures(graph_dir, &metrics_dir, &nodes, &edges, &files)?;
    write_semantic_clusters(graph_dir, &analysis_dir, &metrics_dir, &nodes, &edges, &files)?;

    Ok(ReportParts { nodes, edges, files, cfg, callgraph })
}

fn write_semantic_signatures(
    graph_dir: &Path,
    metrics_dir: &Path,
    nodes: &[CodeGraphNode],
    edges: &[CodeGraphEdge],
    files: &[String],
) -> Result<()> {
    let graph = rows_to_code_graph(nodes, edges, files);
    let features = extract_node_features(graph_dir, &graph)?;
    let _ = compute_signatures(metrics_dir, &features)?;
    Ok(())
}

fn write_semantic_clusters(
    graph_dir: &Path,
    analysis_dir: &Path,
    metrics_dir: &Path,
    nodes: &[CodeGraphNode],
    edges: &[CodeGraphEdge],
    files: &[String],
) -> Result<()> {
    let graph = rows_to_code_graph(nodes, edges, files);
    let features = extract_node_features(graph_dir, &graph)?;
    let clustering = cluster_dbscan_like(&features, 5.0, 3);

    fs::create_dir_all(analysis_dir)?;
    fs::write(
        analysis_dir.join("semantic_clusters.json"),
        serde_json::to_string_pretty(&clustering.clusters)?,
    )?;

    let mut outliers = Vec::new();
    let mut outlier_ids = clustering.outliers;
    outlier_ids.sort_unstable();
    for id in outlier_ids {
        if let Some(node) = graph.nodes.iter().find(|n| n.id == id) {
            outliers.push(serde_json::json!({
                "node_id": node.id,
                "symbol": node.symbol,
                "file": node.file,
                "line": node.line,
                "kind": node.kind,
            }));
        }
    }
    fs::write(
        analysis_dir.join("semantic_outliers.json"),
        serde_json::to_string_pretty(&outliers)?,
    )?;
    write_cluster_graph_bin(metrics_dir, &clustering.clusters)?;
    Ok(())
}

fn write_cluster_graph_bin(
    metrics_dir: &Path,
    clusters: &[crate::semantics::semantic_clustering::SemanticCluster],
) -> Result<()> {
    let mut buf = Vec::with_capacity(8 + clusters.len() * 16);
    buf.extend_from_slice(&(clusters.len() as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    for c in clusters {
        buf.extend_from_slice(&(c.cluster_id as u64).to_le_bytes());
        buf.extend_from_slice(&(c.nodes.len() as u32).to_le_bytes());
        for id in &c.nodes {
            buf.extend_from_slice(&id.to_le_bytes());
        }
    }
    fs::write(metrics_dir.join("cluster_graph.bin"), buf)?;
    Ok(())
}

fn cleanup_legacy_dirs(out_dir: &Path) -> Result<()> {
    for legacy in ["semantics", "reports"] {
        let legacy_dir = out_dir.join(legacy);
        if legacy_dir.exists() {
            fs::remove_dir_all(&legacy_dir)?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct DiagnosticsReport {
    has_block_edges: usize,
    flow_edges: usize,
    call_edges: usize,
    callsite_nodes: usize,
    function_nodes: usize,
    branch_nodes: usize,
    merge_candidate_blocks: usize,
    calls_per_function: f64,
    should_fail: bool,
    fail_reason: String,
}

fn build_diagnostics(nodes: &[CodeGraphNode], edges: &[CodeGraphEdge]) -> DiagnosticsReport {
    let has_block_edges = edges.iter().filter(|e| e.kind == "HAS_BLOCK").count();
    let flow_edges = edges.iter().filter(|e| e.kind == "FLOW").count();
    let call_edges = edges.iter().filter(|e| e.kind == "CALL").count();
    let callsite_nodes = nodes.iter().filter(|n| n.kind == "CALL_SITE").count();
    let function_nodes = nodes
        .iter()
        .filter(|n| n.kind == "FUNCTION" || n.kind == "METHOD")
        .count();

    let mut reasons = Vec::new();
    if function_nodes > 0 && has_block_edges == 0 {
        reasons.push("missing HAS_BLOCK edges");
    }
    if function_nodes > 0 && flow_edges == 0 {
        reasons.push("missing FLOW edges");
    }
    if call_edges > 0 && callsite_nodes == 0 {
        reasons.push("missing CALL_SITE nodes");
    }

    let should_fail = !reasons.is_empty();
    let fail_reason = if should_fail {
        format!("diagnostics gate failed: {}", reasons.join(", "))
    } else {
        String::new()
    };

    DiagnosticsReport {
        has_block_edges,
        flow_edges,
        call_edges,
        callsite_nodes,
        function_nodes,
        branch_nodes: 0,
        merge_candidate_blocks: 0,
        calls_per_function: if function_nodes == 0 { 0.0 } else { call_edges as f64 / function_nodes as f64 },
        should_fail,
        fail_reason,
    }
}

fn write_diagnostics(reports_dir: &Path, diagnostics: &DiagnosticsReport) -> Result<()> {
    fs::create_dir_all(reports_dir)?;
    let path = reports_dir.join("diagnostics.json");
    fs::write(path, serde_json::to_string_pretty(diagnostics)?)?;
    Ok(())
}

fn enrich_diagnostics_with_topology(
    mut diagnostics: DiagnosticsReport,
    cfg_out: &HashMap<u32, Vec<u32>>,
) -> DiagnosticsReport {
    let mut branch_nodes = 0usize;
    let mut seen_succ: HashMap<Vec<u32>, usize> = HashMap::new();
    let mut merge_candidate_blocks = 0usize;

    for outs in cfg_out.values() {
        if outs.len() > 1 {
            branch_nodes += 1;
        }
        let mut key = outs.clone();
        key.sort_unstable();
        if key.len() > 1 {
            let count = seen_succ.entry(key).or_insert(0);
            *count += 1;
            if *count > 1 {
                merge_candidate_blocks += 1;
            }
        }
    }

    diagnostics.branch_nodes = branch_nodes;
    diagnostics.merge_candidate_blocks = merge_candidate_blocks;
    diagnostics
}

fn write_missing_report_placeholders(
    analysis_dir: &Path,
    metrics_dir: &Path,
    diagnostics: &DiagnosticsReport,
) -> Result<()> {
    let payload = serde_json::json!({
        "reason": "missing required edges",
        "diagnostics": diagnostics,
    });
    for name in [
        "branch_pressure_report.json",
        "merge_candidates_report.json",
        "path_redundancy_report.json",
        "reachability_report.json",
    ] {
        let path = metrics_dir.join(name);
        fs::write(path, serde_json::to_string_pretty(&payload)?)?;
    }
    for name in [
        "dependency_cycle_report.json",
        "cycles.json",
        "hotspots.json",
    ] {
        let path = analysis_dir.join(name);
        fs::write(path, serde_json::to_string_pretty(&payload)?)?;
    }
    let structural = metrics_dir.join("structural_hotspots_report.json");
    fs::write(structural, serde_json::to_string_pretty(&payload)?)?;
    Ok(())
}


fn write_callsite_resolution_from_tlog(tlog_path: &Path, reports_dir: &Path) -> Result<()> {
    let mut report = CallsiteResolutionReport::default();
    for event in read_any_events_from_path(tlog_path)? {
        let canon = match event {
            canon_event_store::AnyEvent::Canon(canon) => canon,
            _ => continue,
        };
        let Some(kernel) = extract_rustc_event(&canon) else {
            continue;
        };
        let RustcEvent::CallsiteObserved(canon_types::CallsiteObserved { kind, resolved }) = kernel else {
            continue;
        };
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

fn write_symbol_artifacts_from_tlog(tlog_path: &Path, out_dir: &Path) -> Result<()> {
    let mut symbols: BTreeMap<String, String> = BTreeMap::new();
    let spans_path = out_dir.join("symbol_spans.jsonl");
    let mut spans_file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&spans_path)?;

    for event in read_any_events_from_path(tlog_path)? {
        let canon = match event {
            canon_event_store::AnyEvent::Canon(canon) => canon,
            _ => continue,
        };
        let Some(kernel) = extract_rustc_event(&canon) else {
            continue;
        };
        match kernel {
            RustcEvent::SymbolDefined(canon_types::SymbolDefined { symbol, kind }) => {
                if !symbol.is_empty() && !kind.is_empty() {
                    symbols.insert(symbol, kind);
                }
            }
            RustcEvent::SpanDefined(canon_types::SpanDefined { symbol, file, line, col, lo, hi }) => {
                if symbol.is_empty() {
                    continue;
                }
                let span = serde_json::json!({
                    "symbol_id": symbol,
                    "file": file,
                    "line": line,
                    "col": col,
                    "lo": lo,
                    "hi": hi
                });
                writeln!(spans_file, "{}", serde_json::to_string(&span)?)?;
            }
            _ => {}
        }
    }

    let symbols_path = out_dir.join("symbols.json");
    fs::write(symbols_path, serde_json::to_string_pretty(&symbols)?)?;
    Ok(())
}

#[allow(dead_code)]
fn write_graph_artifacts(
    graph_dir: &Path,
    graphs_dir: &Path,
    nodes: &[CodeGraphNode],
    edges: &[CodeGraphEdge],
    files: &[String],
) -> Result<(Vec<CodeGraphEdge>, Vec<(u32, u32)>)> {
    let mut cfg = extract_cfg_edges(nodes, edges);
    cfg.sort_by(|a, b| {
        a.src
            .cmp(&b.src)
            .then_with(|| a.dst.cmp(&b.dst))
            .then_with(|| a.kind.cmp(&b.kind))
    });

    let mut callgraph = extract_callgraph_edges(nodes, edges);
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

    emit_nodes_csv(graph_dir, nodes)?;
    emit_nodes_full_csv(graph_dir, nodes, files)?;
    emit_nodes_raw_jsonl(graph_dir, nodes, files)?;
    emit_edges_csv(graph_dir, edges)?;
    emit_edges_full_csv(graph_dir, edges, nodes, files)?;
    emit_files_txt(graph_dir, files)?;
    emit_cfg_csv(graphs_dir, &cfg)?;
    emit_cfg_full_csv(graphs_dir, &cfg, nodes, files)?;
    emit_callgraph_csv(graphs_dir, &callgraph, nodes, files)?;
    emit_callgraph_full_csv(graphs_dir, &callgraph, nodes, files)?;
    emit_modulegraph_csv(graphs_dir, &modulegraph, &module_nodes)?;
    emit_typegraph_csv(graphs_dir, &typegraph, nodes, files)?;
    emit_typegraph_full_csv(graphs_dir, &typegraph, nodes, files)?;

    Ok((cfg, callgraph))
}

fn write_report<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let file = fs::File::create(path)?;
    serde_json::to_writer_pretty(file, data)?;
    Ok(())
}

fn graph_fingerprint(nodes: &[CodeGraphNode], edges: &[CodeGraphEdge], files: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for f in files {
        f.hash(&mut hasher);
    }
    for n in nodes {
        n.id.hash(&mut hasher);
        n.kind.hash(&mut hasher);
        n.symbol.hash(&mut hasher);
        n.file_id.hash(&mut hasher);
        n.line.hash(&mut hasher);
    }
    for e in edges {
        e.src.hash(&mut hasher);
        e.dst.hash(&mut hasher);
        e.kind.hash(&mut hasher);
    }
    hasher.finish()
}

fn read_nodes_csv(path: PathBuf) -> Result<Vec<CodeGraphNode>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(7, ',').collect();
        if parts.len() < 7 {
            continue;
        }
        let id: u32 = parts[0].parse().unwrap_or(0);
        let kind = parts[1].to_string();
        let symbol = parts[2].to_string();
        let file_id = parts[3].parse::<u32>().ok();
        let line = parts[4].parse::<u32>().ok();
        out.push(CodeGraphNode { id, kind, symbol, file_id, line });
    }
    Ok(out)
}

fn read_edges_csv(path: PathBuf) -> Result<Vec<CodeGraphEdge>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, ',').collect();
        if parts.len() < 3 {
            continue;
        }
        let src: u32 = parts[0].parse().unwrap_or(0);
        let dst: u32 = parts[1].parse().unwrap_or(0);
        let kind = parts[2].to_string();
        out.push(CodeGraphEdge { src, dst, kind });
    }
    Ok(out)
}

fn read_files_txt(path: PathBuf) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    let mut files = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 2 {
            continue;
        }
        let id = parts[0].parse::<usize>().unwrap_or(usize::MAX);
        if id == usize::MAX {
            continue;
        }
        let path = parts[1..].join(",");
        if files.len() <= id {
            files.resize(id + 1, String::new());
        }
        files[id] = path;
    }
    Ok(files)
}

use crate::artifacts_loader::{load_kernel_graph, KernelGraph};
use crate::invariant_discovery::{discover_invariants, mine_candidates, InvariantResult};
use crate::semantic_features::extract_node_features;
use crate::semantic_signature::compute_signatures;
use crate::semantic_clustering::{cluster_dbscan_like, ClusteringResult};
use crate::pattern_mining::mine_patterns;
use crate::invariant_generator::generate_candidates;
use crate::invariant_sat::validate_candidates;
use crate::report_ingest::{ingest_reports, ReportFeatures};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
struct InvariantRecord {
    invariant: String,
    description: String,
    satisfied: bool,
    coverage: f64,
    violation_rate: f64,
    violations: Vec<u32>,
}

#[derive(Debug, Serialize)]
struct InvariantReport {
    ok: bool,
    invariants: Vec<InvariantRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
struct InvariantHistoryEntry {
    timestamp: u64,
    invariant: String,
    coverage: f64,
    violation_rate: f64,
    satisfied: bool,
}

pub fn run_invariant_pipeline(graph_dir: &Path) -> Result<()> {
    let base_dir = graph_dir.parent().unwrap_or(graph_dir);
    let graph = load_kernel_graph(graph_dir)?;
    let features = ingest_reports(base_dir, &graph)?;
    let invariants = discover_invariants(&graph, &features);
    let report = build_report(&invariants);
    write_report(base_dir, &report)?;
    write_violations(base_dir, &graph, &invariants)?;
    write_discovered(base_dir, &graph, &features)?;
    update_history(base_dir, &invariants)?;
    run_semantic_pipeline(base_dir, graph_dir, &graph)?;
    Ok(())
}

fn build_report(invariants: &[InvariantResult]) -> InvariantReport {
    let records = invariants
        .iter()
        .map(|inv| InvariantRecord {
            invariant: inv.name.clone(),
            description: inv.description.clone(),
            satisfied: inv.violation_rate == 0.0,
            coverage: inv.coverage,
            violation_rate: inv.violation_rate,
            violations: inv.violations.clone(),
        })
        .collect::<Vec<_>>();
    let ok = records.iter().all(|r| r.satisfied);
    InvariantReport { ok, invariants: records }
}

fn write_report(base_dir: &Path, report: &InvariantReport) -> Result<()> {
    let reports_dir = base_dir.join("reports");
    fs::create_dir_all(&reports_dir)?;
    let path = reports_dir.join("upg_invariants.json");
    let payload = serde_json::to_string_pretty(report)?;
    fs::write(path, payload)?;
    Ok(())
}

fn write_discovered(graph_dir: &Path, graph: &KernelGraph, features: &ReportFeatures) -> Result<()> {
    let discovered = mine_candidates(graph, features);
    let path = graph_dir.join("reports").join("invariants_discovered.json");
    fs::create_dir_all(path.parent().unwrap())?;
    let payload = serde_json::to_string_pretty(&discovered)?;
    fs::write(path, payload)?;
    Ok(())
}

fn write_violations(
    graph_dir: &Path,
    graph: &KernelGraph,
    invariants: &[InvariantResult],
) -> Result<()> {
    let mut id_to_node = HashMap::new();
    for n in &graph.nodes {
        id_to_node.insert(n.id, n);
    }
    let out_dir = graph_dir.join("reports").join("invariant_violations");
    fs::create_dir_all(&out_dir)?;
    for inv in invariants {
        if inv.violations.is_empty() {
            continue;
        }
        let mut entries = Vec::new();
        for id in &inv.violations {
            if let Some(node) = id_to_node.get(id) {
                entries.push(serde_json::json!({
                    "node_id": node.id,
                    "symbol": node.symbol,
                    "file": node.file,
                    "line": node.line,
                }));
            }
        }
        let path = out_dir.join(format!("{}.json", inv.name));
        fs::write(path, serde_json::to_string_pretty(&entries)?)?;
    }
    Ok(())
}

fn update_history(graph_dir: &Path, invariants: &[InvariantResult]) -> Result<()> {
    let history_dir = graph_dir.join("invariants");
    fs::create_dir_all(&history_dir)?;
    let path = history_dir.join("invariant_history.json");
    let mut history: Vec<InvariantHistoryEntry> = if path.exists() {
        let data = fs::read_to_string(&path)?;
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    };
    let ts = current_timestamp();
    for inv in invariants {
        history.push(InvariantHistoryEntry {
            timestamp: ts,
            invariant: inv.name.clone(),
            coverage: inv.coverage,
            violation_rate: inv.violation_rate,
            satisfied: inv.violation_rate == 0.0,
        });
    }
    fs::write(path, serde_json::to_string_pretty(&history)?)?;
    Ok(())
}

fn run_semantic_pipeline(base_dir: &Path, graph_dir: &Path, graph: &KernelGraph) -> Result<()> {
    let features = extract_node_features(graph_dir, graph)?;
    let signatures = compute_signatures(graph_dir, &features)?;
    let clustering = cluster_dbscan_like(&features, 5.0, 3);
    let patterns = mine_patterns(&clustering.clusters);
    let candidates = generate_candidates(&patterns);
    let sat = validate_candidates(&candidates);

    let reports_dir = base_dir.join("reports");
    fs::create_dir_all(&reports_dir)?;

    let clusters_path = reports_dir.join("semantic_clusters.json");
    fs::write(clusters_path, serde_json::to_string_pretty(&clustering.clusters)?)?;

    let candidates_path = reports_dir.join("invariant_candidates.json");
    fs::write(candidates_path, serde_json::to_string_pretty(&candidates)?)?;

    let sat_path = reports_dir.join("invariant_validated.json");
    fs::write(sat_path, serde_json::to_string_pretty(&sat)?)?;

    // Outliers: clusters with size 1
    let mut outliers = Vec::new();
    for id in clustering.outliers {
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
    let out_path = reports_dir.join("semantic_outliers.json");
    fs::write(out_path, serde_json::to_string_pretty(&outliers)?)?;

    Ok(())
}
fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

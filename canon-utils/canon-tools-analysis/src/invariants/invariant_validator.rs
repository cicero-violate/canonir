use crate::invariants::invariant_discovery::{discover_invariants, mine_candidates, InvariantResult};
use crate::invariants::invariant_generator::generate_candidates;
use crate::invariants::invariant_sat::validate_candidates;
use crate::semantics::pattern_mining::mine_patterns;
use crate::semantics::semantic_clustering::cluster_dbscan_like;
use crate::semantics::semantic_features::extract_node_features;
use crate::semantics::semantic_signature::compute_signatures;
use anyhow::Result;
use canon_graph::artifacts_loader::{load_code_graph, CodeGraph};
use canon_graph::ingest::report_ingest::{ingest_reports, ReportFeatures};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

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

pub fn run_invariant_pipeline(graph_dir: &Path, invariants_dir: &Path, meta_dir: &Path, _analysis_dir: &Path, metrics_dir: &Path) -> Result<()> {
    let graph = load_code_graph(graph_dir)?;
    let features = ingest_reports(metrics_dir, &graph)?;
    let invariants = discover_invariants(&graph, &features);
    let report = build_report(&invariants);
    write_report(invariants_dir, &report)?;
    write_violations(invariants_dir, &graph, &invariants)?;
    write_discovered(invariants_dir, &graph, &features)?;
    update_history(meta_dir, &invariants)?;
    run_semantic_pipeline(invariants_dir, metrics_dir, graph_dir, &graph)?;
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

fn write_report(invariants_dir: &Path, report: &InvariantReport) -> Result<()> {
    fs::create_dir_all(invariants_dir)?;
    let path = invariants_dir.join("invariant_report.json");
    let payload = serde_json::to_string_pretty(report)?;
    fs::write(path, payload)?;
    Ok(())
}

fn write_discovered(invariants_dir: &Path, graph: &CodeGraph, features: &ReportFeatures) -> Result<()> {
    let discovered = mine_candidates(graph, features);
    fs::create_dir_all(invariants_dir)?;
    let path = invariants_dir.join("invariants_discovered.json");
    let payload = serde_json::to_string_pretty(&discovered)?;
    fs::write(path, payload)?;
    Ok(())
}

fn write_violations(invariants_dir: &Path, graph: &CodeGraph, invariants: &[InvariantResult]) -> Result<()> {
    let mut id_to_node = HashMap::new();
    for n in &graph.nodes {
        id_to_node.insert(n.id, n);
    }
    fs::create_dir_all(invariants_dir)?;
    let mut out = Vec::new();
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
        out.push(serde_json::json!({
            "invariant": inv.name,
            "violations": entries,
        }));
    }
    fs::write(invariants_dir.join("violations.json"), serde_json::to_string_pretty(&out)?)?;
    Ok(())
}

fn update_history(meta_dir: &Path, invariants: &[InvariantResult]) -> Result<()> {
    fs::create_dir_all(meta_dir)?;
    let path = meta_dir.join("history.json");
    let mut history: Vec<InvariantHistoryEntry> = if path.exists() {
        let data = fs::read_to_string(&path)?;
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    };
    let ts = current_timestamp();
    for inv in invariants {
        history.push(InvariantHistoryEntry { timestamp: ts, invariant: inv.name.clone(), coverage: inv.coverage, violation_rate: inv.violation_rate, satisfied: inv.violation_rate == 0.0 });
    }
    fs::write(path, serde_json::to_string_pretty(&history)?)?;
    Ok(())
}

fn run_semantic_pipeline(invariants_dir: &Path, metrics_dir: &Path, graph_dir: &Path, graph: &CodeGraph) -> Result<()> {
    let features = extract_node_features(graph_dir, graph)?;
    let _signatures = compute_signatures(metrics_dir, &features)?;
    let clustering = cluster_dbscan_like(&features, 5.0, 3);
    let patterns = mine_patterns(&clustering.clusters);
    let candidates = generate_candidates(&patterns);
    let sat = validate_candidates(&candidates);

    fs::create_dir_all(invariants_dir)?;
    let candidates_path = invariants_dir.join("invariant_candidates.json");
    fs::write(candidates_path, serde_json::to_string_pretty(&candidates)?)?;

    let sat_path = invariants_dir.join("invariant_validated.json");
    fs::write(sat_path, serde_json::to_string_pretty(&sat)?)?;
    Ok(())
}
fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

use crate::artifacts_loader::KernelGraph;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct FeatureVector {
    pub call_centrality: Option<f64>,
    pub branch_complexity: Option<f64>,
    pub dead_code: bool,
    pub dependency_cycle: bool,
    pub structural_hotspot: bool,
}

#[derive(Debug, Default)]
pub struct ReportFeatures {
    pub node_metrics: HashMap<u32, FeatureVector>,
}

#[derive(Deserialize)]
struct CallgraphCentralityEntry {
    symbol: String,
    centrality_score: usize,
}

#[derive(Deserialize)]
struct BranchComplexityEntry {
    symbol: String,
    score: usize,
}

#[derive(Deserialize)]
struct DeadCodeEntry {
    symbol: String,
}

#[derive(Deserialize)]
struct DependencyCycleEntry {
    nodes: Vec<String>,
}

#[derive(Deserialize)]
struct StructuralHotspotEntry {
    symbol: String,
    score: usize,
}

pub fn ingest_reports(graph_dir: &Path, graph: &KernelGraph) -> Result<ReportFeatures> {
    let mut features = ReportFeatures::default();
    let reports_dir = graph_dir.join("reports");

    ingest_callgraph(&reports_dir, graph, &mut features)?;
    ingest_branch_complexity(&reports_dir, graph, &mut features)?;
    ingest_dead_code(&reports_dir, graph, &mut features)?;
    ingest_dependency_cycles(&reports_dir, graph, &mut features)?;
    ingest_structural_hotspots(&reports_dir, graph, &mut features)?;

    Ok(features)
}

fn ingest_callgraph(
    reports_dir: &Path,
    graph: &KernelGraph,
    features: &mut ReportFeatures,
) -> Result<()> {
    let path = reports_dir.join("callgraph_centrality_report.json");
    if !path.exists() {
        return Ok(());
    }
    let data = fs::read_to_string(path)?;
    let entries: Vec<CallgraphCentralityEntry> = serde_json::from_str(&data).unwrap_or_default();
    for e in entries {
        if let Some(&id) = graph.symbol_to_id.get(&e.symbol) {
            let v = features.node_metrics.entry(id).or_default();
            v.call_centrality = Some(e.centrality_score as f64);
        }
    }
    Ok(())
}

fn ingest_branch_complexity(
    reports_dir: &Path,
    graph: &KernelGraph,
    features: &mut ReportFeatures,
) -> Result<()> {
    let path = reports_dir.join("branch_complexity_report.json");
    if !path.exists() {
        return Ok(());
    }
    let data = fs::read_to_string(path)?;
    let entries: Vec<BranchComplexityEntry> = serde_json::from_str(&data).unwrap_or_default();
    for e in entries {
        if let Some(&id) = graph.symbol_to_id.get(&e.symbol) {
            let v = features.node_metrics.entry(id).or_default();
            v.branch_complexity = Some(e.score as f64);
        }
    }
    Ok(())
}

fn ingest_dead_code(
    reports_dir: &Path,
    graph: &KernelGraph,
    features: &mut ReportFeatures,
) -> Result<()> {
    let path = reports_dir.join("dead_code_report.json");
    if !path.exists() {
        return Ok(());
    }
    let data = fs::read_to_string(path)?;
    let entries: Vec<DeadCodeEntry> = serde_json::from_str(&data).unwrap_or_default();
    for e in entries {
        if let Some(&id) = graph.symbol_to_id.get(&e.symbol) {
            let v = features.node_metrics.entry(id).or_default();
            v.dead_code = true;
        }
    }
    Ok(())
}

fn ingest_dependency_cycles(
    reports_dir: &Path,
    graph: &KernelGraph,
    features: &mut ReportFeatures,
) -> Result<()> {
    let path = reports_dir.join("dependency_cycle_report.json");
    if !path.exists() {
        return Ok(());
    }
    let data = fs::read_to_string(path)?;
    let entries: Vec<DependencyCycleEntry> = serde_json::from_str(&data).unwrap_or_default();
    for e in entries {
        for sym in e.nodes {
            if let Some(&id) = graph.symbol_to_id.get(&sym) {
                let v = features.node_metrics.entry(id).or_default();
                v.dependency_cycle = true;
            }
        }
    }
    Ok(())
}

fn ingest_structural_hotspots(
    reports_dir: &Path,
    graph: &KernelGraph,
    features: &mut ReportFeatures,
) -> Result<()> {
    let path = reports_dir.join("structural_hotspots_report.json");
    if !path.exists() {
        return Ok(());
    }
    let data = fs::read_to_string(path)?;
    let entries: Vec<StructuralHotspotEntry> = serde_json::from_str(&data).unwrap_or_default();
    for e in entries {
        if let Some(&id) = graph.symbol_to_id.get(&e.symbol) {
            let v = features.node_metrics.entry(id).or_default();
            v.structural_hotspot = true;
            if v.branch_complexity.is_none() {
                v.branch_complexity = Some(e.score as f64);
            }
        }
    }
    Ok(())
}

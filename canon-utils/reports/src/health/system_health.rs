use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::health::graph_health::GraphHealthReport;
use crate::health::tlog_integrity::TlogIntegrityReport;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemHealthReport {
    pub graph_drift: f64,
    pub callgraph_ratio: f64,
    pub orphan_nodes: usize,
    pub tlog_growth_rate: f64,
    pub tlog_ok: bool,
    pub generated_at: String,
}

pub fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn write_system_health_report(tlog_path: &Path, reports_dir: &Path) -> Result<()> {
    fs::create_dir_all(reports_dir)?;
    let graph_health_path = reports_dir.join("graph_health.json");
    let tlog_integrity_path = reports_dir.join("tlog_integrity.json");
    let graph_health: Option<GraphHealthReport> = if graph_health_path.exists() {
        serde_json::from_str(&fs::read_to_string(&graph_health_path)?).ok()
    } else {
        None
    };
    let tlog_integrity: Option<TlogIntegrityReport> = if tlog_integrity_path.exists() {
        serde_json::from_str(&fs::read_to_string(&tlog_integrity_path)?).ok()
    } else {
        None
    };

    let tlog_size = fs::metadata(tlog_path).map(|m| m.len()).unwrap_or(0);
    let prev_size = tlog_integrity.as_ref().map(|r| r.file_size).unwrap_or(tlog_size);
    let tlog_growth_rate = if prev_size == 0 {
        0.0
    } else {
        (tlog_size.saturating_sub(prev_size)) as f64 / prev_size as f64
    };

    let report = SystemHealthReport {
        graph_drift: graph_health.as_ref().map(|r| r.graph_drift).unwrap_or(0.0),
        callgraph_ratio: graph_health.as_ref().map(|r| r.callgraph_ratio).unwrap_or(0.0),
        orphan_nodes: graph_health.as_ref().map(|r| r.orphan_nodes).unwrap_or(0),
        tlog_growth_rate,
        tlog_ok: tlog_integrity.as_ref().map(|r| r.replay_determinism_ok).unwrap_or(false),
        generated_at: current_timestamp().to_string(),
    };
    fs::write(reports_dir.join("system_health.json"), serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

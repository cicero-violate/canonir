use super::shell::run_capture;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn append_report_line(path: &str, payload: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let mut line = serde_json::to_string(payload)?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    Ok(())
}

pub(crate) fn git_head_commit(project: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let out = run_capture(project, "git", &["rev-parse", "HEAD"])?;
    Ok(out.trim().to_string())
}

pub(crate) fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(crate) fn now_iso_utc() -> String {
    let now = chrono::Utc::now();
    now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(crate) fn now_compact_utc() -> String {
    let now = chrono::Utc::now();
    now.format("%Y%m%dT%H%M%SZ").to_string()
}

#[derive(Default)]
pub(crate) struct KindStats {
    pub(crate) attempts: usize,
    pub(crate) accepted: usize,
    pub(crate) introduced_errors: usize,
}

pub(crate) fn update_kind_stats(
    stats: &mut BTreeMap<String, KindStats>,
    symbol_kind: &str,
    accepted: bool,
    introduced_errors: usize,
) {
    let entry = stats.entry(symbol_kind.to_string()).or_insert_with(KindStats::default);
    entry.attempts += 1;
    if accepted {
        entry.accepted += 1;
    } else if introduced_errors > 0 {
        entry.introduced_errors += 1;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SolverPlan {
    pub(crate) input_total: usize,
    pub(crate) transform_total: usize,
    pub(crate) dependency_count: usize,
    pub(crate) conflict_count: usize,
    pub(crate) cyclic_component_count: usize,
    pub(crate) sat_selected_total: usize,
    pub(crate) selected_total: usize,
    pub(crate) selected_pairs: Vec<(String, String)>,
}

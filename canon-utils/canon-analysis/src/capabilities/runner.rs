use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::sync::Mutex;
use std::path::PathBuf;

static RUN_GUARD: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

pub enum RunOutcome {
    Ran(PathBuf),
    Skipped(PathBuf),
}

pub fn run_full_analysis(args: &serde_json::Value) -> Result<RunOutcome> {
    let crate_name = args
        .get("crate")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing crate in capability args"))?;
    let batch_id = args
        .get("batch_id")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    let reports_root = if let Some(root) = args.get("reports_root").and_then(|v| v.as_str()) {
        PathBuf::from(root)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("state")
            .join("reports_out")
    };

    let crate_root = reports_root.join("crates").join(crate_name);
    let guard_key = format!("{crate_name}:{batch_id}");
    if let Ok(mut guard) = RUN_GUARD.lock() {
        if guard.contains(&guard_key) {
            return Ok(RunOutcome::Skipped(crate_root));
        }
        guard.insert(guard_key);
    }

    let tlog_path = crate::capabilities::events::resolve_tlog_path();
    crate::report_pipeline::generate_reports_from_tlog(&tlog_path, &crate_root)?;
    Ok(RunOutcome::Ran(crate_root))
}

pub fn ensure_workspace_root(args: &serde_json::Value) -> Result<PathBuf> {
    if let Some(path) = args.get("workspace").and_then(|v| v.as_str()) {
        return Ok(PathBuf::from(path));
    }
    Ok(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

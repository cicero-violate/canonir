use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::hash::{Hash, Hasher};

pub fn aggregate_workspace(reports_root: &Path) -> Result<()> {
    let crates_dir = reports_root.join("crates");
    let workspace_dir = reports_root.join("workspace");
    fs::create_dir_all(&workspace_dir)?;

    let mut callgraph_rows: Vec<String> = Vec::new();
    let mut cycles: Vec<Value> = Vec::new();
    let mut invariant_reports: Vec<Value> = Vec::new();
    let mut violations: Vec<Value> = Vec::new();
    let mut history: Vec<Value> = Vec::new();
    let mut input_files: Vec<PathBuf> = Vec::new();

    if crates_dir.exists() {
        for entry in fs::read_dir(&crates_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let crate_name = entry.file_name().to_string_lossy().to_string();
            aggregate_callgraph(&crate_name, &path, &mut callgraph_rows, &mut input_files);
            aggregate_dependency_cycles(&crate_name, &path, &mut cycles, &mut input_files);
            aggregate_invariants(&crate_name, &path, &mut invariant_reports, &mut violations, &mut input_files);
            aggregate_history(&crate_name, &path, &mut history, &mut input_files);
        }
    }

    write_global_callgraph(&workspace_dir, &callgraph_rows)?;
    fs::write(
        workspace_dir.join("global_dependency_cycles.json"),
        serde_json::to_string_pretty(&cycles)?,
    )?;
    fs::write(
        workspace_dir.join("global_invariant_report.json"),
        serde_json::to_string_pretty(&serde_json::json!({ "crates": invariant_reports }))?,
    )?;
    fs::write(
        workspace_dir.join("global_violations.json"),
        serde_json::to_string_pretty(&serde_json::json!({ "crates": violations }))?,
    )?;
    fs::write(
        workspace_dir.join("history.json"),
        serde_json::to_string_pretty(&history)?,
    )?;
    let fingerprint = input_fingerprint(&input_files)?;
    let meta = serde_json::json!({
        "crates_dir": crates_dir,
        "input_count": input_files.len(),
        "fingerprint": fingerprint
    });
    fs::write(
        workspace_dir.join("aggregation_meta.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;

    Ok(())
}

fn aggregate_callgraph(crate_name: &str, crate_dir: &Path, out: &mut Vec<String>, inputs: &mut Vec<PathBuf>) {
    let path = crate_dir.join("graphs").join("callgraph_full.csv");
    if !path.exists() {
        return;
    }
    inputs.push(path.clone());
    if let Ok(content) = fs::read_to_string(&path) {
        for (idx, line) in content.lines().enumerate() {
            if idx == 0 || line.trim().is_empty() {
                continue;
            }
            let parts = line.split(',').collect::<Vec<_>>();
            if parts.len() < 6 {
                continue;
            }
            // columns: caller_node,callee_node,caller_symbol,callee_symbol,caller_file,callee_file
            let caller_symbol = parts.get(2).copied().unwrap_or("");
            let callee_symbol = parts.get(3).copied().unwrap_or("");
            let caller_file = parts.get(4).copied().unwrap_or("");
            let callee_file = parts.get(5).copied().unwrap_or("");
            out.push(format!(
                "{},{},{},{},{}",
                crate_name, caller_symbol, callee_symbol, caller_file, callee_file
            ));
        }
    }
}

fn aggregate_dependency_cycles(
    crate_name: &str,
    crate_dir: &Path,
    out: &mut Vec<Value>,
    inputs: &mut Vec<PathBuf>,
) {
    let path = crate_dir
        .join("analysis")
        .join("dependency_cycle_report.json");
    if let Ok(text) = fs::read_to_string(&path) {
        inputs.push(path.clone());
        if let Ok(val) = serde_json::from_str::<Value>(&text) {
            out.push(serde_json::json!({
                "crate": crate_name,
                "cycles": val
            }));
        }
    }
}

fn aggregate_invariants(
    crate_name: &str,
    crate_dir: &Path,
    reports: &mut Vec<Value>,
    violations: &mut Vec<Value>,
    inputs: &mut Vec<PathBuf>,
) {
    let report_path = crate_dir.join("invariants").join("invariant_report.json");
    if let Ok(text) = fs::read_to_string(&report_path) {
        inputs.push(report_path.clone());
        if let Ok(val) = serde_json::from_str::<Value>(&text) {
            reports.push(serde_json::json!({
                "crate": crate_name,
                "report": val
            }));
        }
    }
    let violations_path = crate_dir.join("invariants").join("violations.json");
    if let Ok(text) = fs::read_to_string(&violations_path) {
        inputs.push(violations_path.clone());
        if let Ok(val) = serde_json::from_str::<Value>(&text) {
            violations.push(serde_json::json!({
                "crate": crate_name,
                "violations": val
            }));
        }
    }
}

fn aggregate_history(crate_name: &str, crate_dir: &Path, out: &mut Vec<Value>, inputs: &mut Vec<PathBuf>) {
    let path = crate_dir.join("meta").join("history.json");
    if let Ok(text) = fs::read_to_string(&path) {
        inputs.push(path.clone());
        if let Ok(val) = serde_json::from_str::<Value>(&text) {
            if let Some(items) = val.as_array() {
                for item in items {
                    out.push(serde_json::json!({
                        "crate": crate_name,
                        "entry": item
                    }));
                }
            }
        }
    }
}

fn write_global_callgraph(workspace_dir: &Path, rows: &[String]) -> Result<()> {
    let mut buf = String::from("crate,caller_symbol,callee_symbol,caller_file,callee_file\n");
    for row in rows {
        buf.push_str(row);
        buf.push('\n');
    }
    fs::write(workspace_dir.join("global_callgraph.csv"), buf)?;
    Ok(())
}

fn input_fingerprint(paths: &[PathBuf]) -> Result<String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for path in paths {
        path.to_string_lossy().hash(&mut hasher);
        if let Ok(data) = fs::read(path) {
            data.hash(&mut hasher);
        }
    }
    Ok(format!("{:016x}", hasher.finish()))
}

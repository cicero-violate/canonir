use anyhow::Result;
use canon_analysis::report_pipeline::{generate_reports_for_crate, generate_reports_from_tlog};
use canon_types::ReportLayout;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct ReportIndex {
    workspace: String,
    tlog: String,
    crate_name: Option<String>,
    out_dir: String,
    metrics_dir: String,
    analysis_dir: String,
    graph_dir: String,
    reports: Vec<String>,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let workspace = arg_value(&args, "--workspace").unwrap_or("/workspace/ai_sandbox/canon".to_string());
    let tlog = arg_value(&args, "--tlog")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(workspace.clone()).join("state/event_log/event.tlog.d"));
    let crate_name = arg_value(&args, "--crate");
    let out_dir = arg_value(&args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_out_dir(Path::new(&workspace), crate_name.as_deref()));

    if let Some(name) = crate_name.as_deref() {
        generate_reports_for_crate(&tlog, &out_dir, name)?;
    } else {
        generate_reports_from_tlog(&tlog, &out_dir)?;
    }

    let layout = if crate_name.is_some() {
        ReportLayout::from_crate_root(out_dir.clone())
    } else {
        ReportLayout::from_direct_root(out_dir.clone())
    };
    let metrics_dir = layout.metrics_dir();
    let analysis_dir = layout.analysis_dir();
    let graph_dir = layout.graph_dir();

    let mut reports = Vec::new();
    reports.extend(collect_reports(&metrics_dir)?);
    reports.extend(collect_reports(&analysis_dir)?);

    let payload = serde_json::to_string_pretty(&ReportIndex {
        workspace,
        tlog: tlog.to_string_lossy().to_string(),
        crate_name,
        out_dir: out_dir.to_string_lossy().to_string(),
        metrics_dir: metrics_dir.to_string_lossy().to_string(),
        analysis_dir: analysis_dir.to_string_lossy().to_string(),
        graph_dir: graph_dir.to_string_lossy().to_string(),
        reports,
    })?;
    println!("{payload}");
    Ok(())
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].to_string())
}

fn default_out_dir(workspace: &Path, crate_name: Option<&str>) -> PathBuf {
    if let Some(name) = crate_name {
        workspace.join("state/reports_out/crates").join(name)
    } else {
        workspace.join("state/reports_out")
    }
}

fn collect_reports(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "json" || ext == "csv" {
                out.push(path.to_string_lossy().to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

use anyhow::{anyhow, Result};
use canon_analysis::report_pipeline::generate_reports_for_crate;
use canon_event_store::{extract_rustc_event, read_any_events_from_path, AnyEvent};
use canon_event::EventKind;
use canon_types::RustcEvent;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let workspace = arg_value(&args, "--workspace").unwrap_or("/workspace/ai_sandbox/canon".to_string());
    let tlog = arg_value(&args, "--tlog")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&workspace).join("state/event_log/event.tlog.d"));

    let crate_name = if let Some(name) = arg_value(&args, "--crate") {
        name
    } else if let Some(path) = arg_value(&args, "--crate-path") {
        crate_name_from_path(Path::new(&path))?
    } else {
        return Err(anyhow!("missing --crate or --crate-path"));
    };

    let out_dir = arg_value(&args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&workspace).join("state/reports_out").join(&crate_name));

    let crates_in_tlog = list_crates_in_tlog(&tlog)?;
    if !crates_in_tlog.contains(&crate_name) {
        return Err(anyhow!(
            "crate '{crate_name}' not found in tlog; available: {}",
            crates_in_tlog.join(", ")
        ));
    }

    generate_reports_for_crate(&tlog, &out_dir, &crate_name)?;
    let graph_bin = out_dir.join("graph").join("graph.bin");
    if !graph_bin.exists() {
        return Err(anyhow!(
            "graph.bin not generated for crate '{crate_name}' (output dir: {})",
            out_dir.display()
        ));
    }
    println!("graph.bin: {}", graph_bin.display());
    Ok(())
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].to_string())
}

fn crate_name_from_path(crate_path: &Path) -> Result<String> {
    let manifest = crate_path.join("Cargo.toml");
    let contents = fs::read_to_string(&manifest)
        .map_err(|e| anyhow!("failed to read {}: {}", manifest.display(), e))?;
    let mut in_package = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name") {
            let rest = rest.trim();
            if let Some(value) = rest.strip_prefix('=').map(str::trim) {
                let name = value.trim_matches('"');
                if !name.is_empty() {
                    // Cargo package name uses '-', rustc crate-name uses '_'
                    return Ok(name.replace('-', "_"));
                }
            }
        }
    }
    Err(anyhow!("package name not found in {}", manifest.display()))
}

fn list_crates_in_tlog(tlog_path: &Path) -> Result<Vec<String>> {
    let mut crates = std::collections::BTreeSet::new();
    for event in read_any_events_from_path(tlog_path)? {
        let AnyEvent::Canon(canon) = event else {
            continue;
        };
        if let Some(kernel) = extract_rustc_event(&canon) {
            match kernel {
                RustcEvent::SessionStart(e) => {
                    crates.insert(e.project);
                }
                RustcEvent::CompilationUnitFinished(e) => {
                    crates.insert(e.crate_name);
                }
                RustcEvent::NodeDefined(e) => {
                    if !e.symbol.is_empty() {
                        // best-effort: symbols are fully qualified; pull crate name prefix
                        if let Some((head, _)) = e.symbol.split_once("::") {
                            crates.insert(head.to_string());
                        }
                    }
                }
                _ => {}
            }
            continue;
        }

        match canon.kind {
            EventKind::RustcCaptureStarted
            | EventKind::RustcCaptureCompleted
            | EventKind::RustcCaptureFailed
            | EventKind::RustcGraphArtifactWritten => {
                if let Some(name) = canon
                    .payload
                    .data
                    .get("crate_name")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string())
                {
                    crates.insert(name);
                } else if let Some(name) = canon
                    .payload
                    .data
                    .get("project")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string())
                {
                    crates.insert(name);
                }
            }
            _ => {}
        }
    }
    Ok(crates.into_iter().collect())
}

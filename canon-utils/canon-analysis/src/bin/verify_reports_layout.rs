use anyhow::{anyhow, Result};
use std::env;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let mut root: Option<PathBuf> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--reports-root" => {
                let value = args.next().ok_or_else(|| anyhow!("--reports-root requires a path"))?;
                root = Some(PathBuf::from(value));
            }
            _ => return Err(anyhow!("unknown argument: {}", arg)),
        }
    }
    let root = root.unwrap_or_else(|| PathBuf::from("state/reports_out"));
    verify_layout(&root)?;
    Ok(())
}

fn verify_layout(root: &Path) -> Result<()> {
    let legacy_kernel = root.join("kernel");
    if legacy_kernel.exists() {
        return Err(anyhow!("legacy layout detected: {}", legacy_kernel.display()));
    }
    let legacy_reports = root.join("reports");
    if legacy_reports.exists() {
        return Err(anyhow!("legacy layout detected: {}", legacy_reports.display()));
    }

    let crates_dir = root.join("crates");
    if !crates_dir.exists() {
        return Err(anyhow!("missing crates dir: {}", crates_dir.display()));
    }
    let workspace_dir = root.join("workspace");
    if !workspace_dir.exists() {
        return Err(anyhow!("missing workspace dir: {}", workspace_dir.display()));
    }

    for entry in std::fs::read_dir(&crates_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        for name in ["graph", "graphs", "analysis", "metrics", "invariants", "meta"] {
            let child = path.join(name);
            if !child.exists() {
                return Err(anyhow!(
                    "crate {} missing {}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    name
                ));
            }
        }
    }
    Ok(())
}

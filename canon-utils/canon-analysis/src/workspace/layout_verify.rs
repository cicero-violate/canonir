use anyhow::{anyhow, Result};
use std::path::Path;

pub fn verify_reports_layout(root: &Path) -> Result<()> {
    let legacy_rustc = root.join("rustc");
    if legacy_rustc.exists() {
        return Err(anyhow!("legacy layout detected: {}", legacy_rustc.display()));
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

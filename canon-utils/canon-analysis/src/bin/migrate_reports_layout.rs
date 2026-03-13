use anyhow::{anyhow, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let mut root: Option<PathBuf> = None;
    let mut default_crate = "canon_kernel".to_string();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--reports-root" => {
                let value = args.next().ok_or_else(|| anyhow!("--reports-root requires a path"))?;
                root = Some(PathBuf::from(value));
            }
            "--default-crate" => {
                default_crate = args.next().ok_or_else(|| anyhow!("--default-crate requires a value"))?;
            }
            _ => return Err(anyhow!("unknown argument: {}", arg)),
        }
    }
    let root = root.unwrap_or_else(|| PathBuf::from("state/reports_out"));
    migrate_reports_layout(&root, &default_crate)?;
    Ok(())
}

fn migrate_reports_layout(root: &Path, default_crate: &str) -> Result<()> {
    let crates_dir = root.join("crates");
    let workspace_dir = root.join("workspace");
    fs::create_dir_all(&crates_dir)?;
    fs::create_dir_all(&workspace_dir)?;

    let legacy_kernel = root.join("kernel");
    if legacy_kernel.exists() {
        let legacy_crates = legacy_kernel.join("crates");
        if legacy_crates.exists() {
            for entry in fs::read_dir(&legacy_crates)? {
                let entry = entry?;
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let dest = crates_dir.join(&name);
                if dest.exists() {
                    continue;
                }
                fs::rename(&path, &dest)?;
            }
        } else {
            let dest = crates_dir.join(default_crate);
            if !dest.exists() {
                fs::rename(&legacy_kernel, &dest)?;
            }
        }
    }

    let legacy_reports = root.join("reports");
    if legacy_reports.exists() {
        let dest = crates_dir.join(default_crate);
        fs::create_dir_all(&dest)?;
        fs::rename(&legacy_reports, dest.join("analysis"))?;
    }

    cleanup_empty_dir(root.join("kernel"));
    cleanup_empty_dir(root.join("reports"));
    cleanup_empty_dir(root.join("kernel").join("crates"));
    cleanup_empty_dir(root.join("kernel").join("workspace"));
    cleanup_empty_dir(root.join("kernel"));
    Ok(())
}

fn cleanup_empty_dir(path: PathBuf) {
    if let Ok(entries) = fs::read_dir(&path) {
        if entries.count() == 0 {
            let _ = fs::remove_dir(&path);
        }
    }
}

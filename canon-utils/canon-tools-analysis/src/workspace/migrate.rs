use anyhow::Result;
use std::fs;
use std::path::Path;

pub fn migrate_reports_layout(root: &Path, default_crate: &str) -> Result<()> {
    let crates_dir = root.join("crates");
    let workspace_dir = root.join("workspace");
    fs::create_dir_all(&crates_dir)?;
    fs::create_dir_all(&workspace_dir)?;

    let legacy_rustc = root.join("rustc");
    if legacy_rustc.exists() {
        let legacy_crates = legacy_rustc.join("crates");
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
                fs::rename(&legacy_rustc, &dest)?;
            }
        }
    }

    let legacy_reports = root.join("reports");
    if legacy_reports.exists() {
        let dest = crates_dir.join(default_crate);
        fs::create_dir_all(&dest)?;
        fs::rename(&legacy_reports, dest.join("analysis"))?;
    }

    cleanup_empty_dir(&root.join("rustc").join("crates"));
    cleanup_empty_dir(&root.join("rustc").join("workspace"));
    cleanup_empty_dir(&root.join("rustc"));
    cleanup_empty_dir(&root.join("reports"));
    Ok(())
}

fn cleanup_empty_dir(path: &Path) {
    if let Ok(entries) = fs::read_dir(path) {
        if entries.count() == 0 {
            let _ = fs::remove_dir(path);
        }
    }
}

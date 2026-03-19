use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
//
pub fn collect_rs_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|e| e.to_str()) == Some("rs") {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
}

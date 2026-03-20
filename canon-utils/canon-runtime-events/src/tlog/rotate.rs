use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct RotateConfig {
    pub max_bytes: u64,
    pub max_files: usize,
}

impl Default for RotateConfig {
    fn default() -> Self {
        Self { max_bytes: 0, max_files: 5 }
    }
}

pub fn maybe_rotate(path: &Path, config: &RotateConfig) -> io::Result<()> {
    if config.max_bytes == 0 {
        return Ok(());
    }
    let size = match fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(_) => return Ok(()),
    };
    if size < config.max_bytes {
        return Ok(());
    }

    for idx in (1..=config.max_files).rev() {
        let src = rotated_path(path, idx);
        let dst = rotated_path(path, idx + 1);
        if src.exists() {
            let _ = fs::rename(&src, &dst);
        }
    }
    let first = rotated_path(path, 1);
    let _ = fs::rename(path, &first);
    Ok(())
}

fn rotated_path(base: &Path, idx: usize) -> PathBuf {
    let mut name = base.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{idx}"));
    base.with_file_name(name)
}

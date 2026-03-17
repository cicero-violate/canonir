use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

pub fn write_json<T: Serialize>(dir: &Path, name: &str, value: &T) -> Result<()> {
    let path = dir.join(name);
    let content = serde_json::to_string_pretty(value)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn output_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}

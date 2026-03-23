use anyhow::Result;
use std::fs;
use std::path::Path;

pub use canon_types::RustcState;

pub fn load_rustc_state(path: &Path) -> Result<RustcState> {
    let data = fs::read_to_string(path)?;
    let state = serde_json::from_str(&data)?;
    Ok(state)
}

pub fn persist_rustc_state(path: &Path, state: &RustcState) -> Result<()> {
    let data = serde_json::to_string_pretty(state)?;
    fs::write(path, data)?;
    Ok(())
}

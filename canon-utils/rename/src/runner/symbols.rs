use crate::core::rustc_session::RustcSession;
use std::path::Path;

pub(crate) fn parse_symbols_json(path: String) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(&path)?;
    let entries: Vec<serde_json::Value> = serde_json::from_str(&content)?;
    let mut pairs = Vec::new();
    for entry in &entries {
        if entry.get("kind").and_then(|v| v.as_str()) == Some("file") {
            continue;
        }
        if entry.get("rename_safe").and_then(|v| v.as_bool()) == Some(false) {
            continue;
        }
        let old = entry
            .get("old")
            .and_then(|v| v.as_str())
            .or_else(|| entry.get("symbol_id").and_then(|v| v.as_str()))
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "missing 'old' or 'symbol_id' field")
            })?;
        let new = entry
            .get("new")
            .and_then(|v| v.as_str())
            .or_else(|| entry.get("new_name").and_then(|v| v.as_str()))
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "missing 'new' or 'new_name' field")
            })?;
        pairs.push((old.to_string(), new.to_string()));
    }
    Ok(pairs)
}

pub(crate) fn load_symbol_ids(session: &RustcSession) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    Ok(session.symbol_catalog())
}

pub(crate) fn load_symbols_entries(path: &Path) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str::<Vec<serde_json::Value>>(&content)?)
}

pub(crate) fn write_symbols_entries(path: &Path, entries: &[serde_json::Value]) -> Result<(), Box<dyn std::error::Error>> {
    let content = serde_json::to_string_pretty(entries)?;
    std::fs::write(path, format!("{content}\n"))?;
    Ok(())
}

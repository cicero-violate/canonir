use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
//
pub fn start_watcher(tx: Sender<PathBuf>, watch_dirs: &[PathBuf]) -> NotifyResult<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)) {
                for path in event.paths {
                    if is_interesting_path(&path) {
                        let _ = tx.send(path);
                    }
                }
            }
        }
    })?;
    for dir in watch_dirs {
        watcher.watch(dir, RecursiveMode::Recursive)?;
    }
    Ok(watcher)
}

fn is_interesting_path(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        if name == "Cargo.toml" {
            return true;
        }
    }
    matches!(path.extension().and_then(|s| s.to_str()), Some("rs"))
}

pub fn crate_for_path(path: &Path) -> Option<String> {
    let mut cur = path.to_path_buf();
    loop {
        if cur.join("Cargo.toml").is_file() {
            let text = std::fs::read_to_string(cur.join("Cargo.toml")).ok()?;
            let value: toml::Value = toml::from_str(&text).ok()?;
            return value
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
        if !(cur.pop()) {
            break;
        }
    }
    None
}

pub fn affected_crates(paths: &[PathBuf]) -> HashSet<String> {
    let mut crates = HashSet::new();
    for path in paths {
        if let Some(name) = crate_for_path(path) {
            crates.insert(name);
            continue;
        }
        if path.starts_with("/workspace/ai_sandbox/canon/canon-agent-prompts") {
            crates.insert("canon-agent-v3".to_string());
        }
    }
    crates
}

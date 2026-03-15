use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use std::{fs, io::Write};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
struct GuardEntry {
    in_flight: bool,
    last_run: Option<Instant>,
}

static RUN_GUARD: Lazy<Mutex<HashMap<String, GuardEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn analysis_cooldown() -> Duration {
    let cooldown_ms = std::env::var("CANON_ANALYSIS_COOLDOWN_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(2000);
    Duration::from_millis(cooldown_ms)
}

pub enum RunOutcome {
    Ran(PathBuf),
    Skipped(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "format", rename_all = "snake_case")]
enum TlogCursor {
    Jsonl { size: u64 },
    Binary { max_seq: u64, max_seq_size: u64 },
}

fn current_tlog_cursor(tlog_path: &Path) -> Option<TlogCursor> {
    if tlog_path.is_dir() {
        let mut max_seq: Option<u64> = None;
        let mut max_seq_size: u64 = 0;
        for entry in fs::read_dir(tlog_path).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("log") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(seq) = stem.parse::<u64>() else {
                continue;
            };
            let size = path.metadata().ok().map(|m| m.len()).unwrap_or(0);
            if max_seq.map(|current| seq > current).unwrap_or(true) {
                max_seq = Some(seq);
                max_seq_size = size;
            } else if max_seq == Some(seq) {
                max_seq_size = max_seq_size.max(size);
            }
        }
        let seq = max_seq?;
        Some(TlogCursor::Binary {
            max_seq: seq,
            max_seq_size,
        })
    } else {
        let size = tlog_path.metadata().ok()?.len();
        Some(TlogCursor::Jsonl { size })
    }
}

fn tlog_cursor_path(crate_root: &Path) -> PathBuf {
    crate_root.join("meta").join("tlog_cursor.json")
}

fn read_tlog_cursor(path: &Path) -> Option<TlogCursor> {
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_tlog_cursor(path: &Path, cursor: &TlogCursor) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec_pretty(cursor)?;
    let mut file = fs::File::create(path)?;
    file.write_all(&data)?;
    Ok(())
}

pub fn run_full_analysis(args: &serde_json::Value) -> Result<RunOutcome> {
    let crate_name = args
        .get("crate")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing crate in capability args"))?;
    let reports_root = if let Some(root) = args.get("reports_root").and_then(|v| v.as_str()) {
        PathBuf::from(root)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("state")
            .join("reports_out")
    };

    let crate_root = reports_root.join("crates").join(crate_name);
    let tlog_path = canon_event_emit::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG"));
    let cursor_path = tlog_cursor_path(&crate_root);
    if let (Some(current), Some(last)) =
        (current_tlog_cursor(&tlog_path), read_tlog_cursor(&cursor_path))
    {
        if current == last {
            return Ok(RunOutcome::Skipped(crate_root));
        }
    }

    let guard_key = crate_name.to_string();
    let cooldown = analysis_cooldown();
    let now = Instant::now();
    if let Ok(mut guard) = RUN_GUARD.lock() {
        let entry = guard.entry(guard_key.clone()).or_default();
        if entry.in_flight {
            return Ok(RunOutcome::Skipped(crate_root));
        }
        if let Some(last_run) = entry.last_run {
            if now.duration_since(last_run) < cooldown {
                return Ok(RunOutcome::Skipped(crate_root));
            }
        }
        entry.in_flight = true;
    }

    let result = crate::report_pipeline::generate_reports_from_tlog(&tlog_path, &crate_root);
    if let Ok(mut guard) = RUN_GUARD.lock() {
        if let Some(entry) = guard.get_mut(&guard_key) {
            entry.in_flight = false;
            entry.last_run = Some(Instant::now());
        }
    }
    result?;
    if let Some(cursor) = current_tlog_cursor(&tlog_path) {
        write_tlog_cursor(&cursor_path, &cursor)?;
    }
    Ok(RunOutcome::Ran(crate_root))
}

pub fn ensure_workspace_root(args: &serde_json::Value) -> Result<PathBuf> {
    if let Some(path) = args.get("workspace").and_then(|v| v.as_str()) {
        return Ok(PathBuf::from(path));
    }
    Ok(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

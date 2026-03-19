use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::{fs, io::Write};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
struct GuardEntry {
    in_flight: bool,
    last_run: Option<Instant>,
}

static RUN_GUARD: Lazy<Mutex<HashMap<String, GuardEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReportsMeta {
    tlog_cursor: TlogCursor,
    tlog_last_modified: u64,
    report_generated_at: u64,
}

fn reports_meta_path(reports_root: &Path) -> PathBuf {
    reports_root.join("meta").join("report_freshness.json")
}

fn read_reports_meta(path: &Path) -> Option<ReportsMeta> {
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_reports_meta(path: &Path, meta: &ReportsMeta) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec_pretty(meta)?;
    let mut file = fs::File::create(path)?;
    file.write_all(&data)?;
    Ok(())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn path_modified_ts(path: &Path) -> Option<u64> {
    let meta = fs::metadata(path).ok()?;
    meta.modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn tlog_last_modified(tlog_path: &Path) -> Option<u64> {
    if tlog_path.is_dir() {
        let mut latest: Option<u64> = None;
        for entry in fs::read_dir(tlog_path).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("log") {
                continue;
            }
            if let Some(ts) = path_modified_ts(&path) {
                latest = Some(latest.map(|prev: u64| prev.max(ts)).unwrap_or(ts));
            }
        }
        return latest.or_else(|| path_modified_ts(tlog_path));
    }
    path_modified_ts(tlog_path)
}

fn invalidate_reports_if_stale(reports_root: &Path, tlog_path: &Path) -> Result<()> {
    let cursor = match current_tlog_cursor(tlog_path) {
        Some(cursor) => cursor,
        None => return Ok(()),
    };
    let meta_path = reports_meta_path(reports_root);
    let stale = read_reports_meta(&meta_path)
        .map(|meta| meta.tlog_cursor != cursor)
        .unwrap_or(true);
    if stale {
        if reports_root.exists() {
            fs::remove_dir_all(reports_root)?;
        }
        fs::create_dir_all(reports_root)?;
    }
    Ok(())
}

fn collect_crate_names_from_tlog(tlog_path: &Path) -> Result<Vec<String>> {
    let mut crates = BTreeSet::new();
    let events = canon_event_store::read_any_events_from_path(tlog_path)?;
    for event in events {
        let canon_event_store::AnyEvent::Canon(canon) = event else {
            continue;
        };
        let Some(kernel) = canon_event_store::extract_rustc_event(&canon) else {
            continue;
        };
        if let canon_event::RustcEvent::SessionStart(session) = kernel {
            if !session.project.is_empty() {
                crates.insert(session.project);
            }
        }
    }
    Ok(crates.into_iter().collect())
}

pub fn run_full_analysis(args: &serde_json::Value) -> Result<RunOutcome> {
    let crate_name = args
        .get("crate")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing crate in capability args"))?;
    let reports_root = if let Some(root) = args.get("reports_root").and_then(|v| v.as_str()) {
        PathBuf::from(root)
    } else if let Ok(root) = std::env::var("CANON_REPORTS_OUT") {
        PathBuf::from(root)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("state")
            .join("reports_out")
    };

    let crate_root = reports_root.join("crates").join(crate_name);
    let tlog_path = canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG"));

    let guard_key = crate_name.to_string();
    if let Ok(mut guard) = RUN_GUARD.lock() {
        let entry = guard.entry(guard_key.clone()).or_default();
        if entry.in_flight {
            return Ok(RunOutcome::Skipped(crate_root));
        }
        entry.in_flight = true;
    }

    let result = (|| -> Result<()> {
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let cursor_before = current_tlog_cursor(&tlog_path)
                .ok_or_else(|| anyhow!("failed to read tlog cursor"))?;
            let tlog_modified_before = tlog_last_modified(&tlog_path).unwrap_or(0);
            invalidate_reports_if_stale(&reports_root, &tlog_path)?;
            let workspace_dir = reports_root.join("workspace");
            crate::report_pipeline::generate_reports_from_tlog(&tlog_path, &workspace_dir)?;
            crate::report_pipeline::generate_reports_for_crate(&tlog_path, &crate_root, crate_name)?;
            let crates = collect_crate_names_from_tlog(&tlog_path)?;
            for name in crates {
                if name == crate_name {
                    continue;
                }
                let root = reports_root.join("crates").join(&name);
                crate::report_pipeline::generate_reports_for_crate(&tlog_path, &root, &name)?;
            }
            crate::workspace::aggregator::aggregate_workspace(&reports_root)?;
            let report_generated_at = unix_timestamp();
            let cursor_after = current_tlog_cursor(&tlog_path)
                .ok_or_else(|| anyhow!("failed to read tlog cursor"))?;
            let tlog_modified_after = tlog_last_modified(&tlog_path).unwrap_or(0);
            if cursor_before != cursor_after || tlog_modified_after > report_generated_at {
                if attempts < 2 {
                    continue;
                }
                return Err(anyhow!("tlog changed during report generation"));
            }
            let meta = ReportsMeta {
                tlog_cursor: cursor_after,
                tlog_last_modified: tlog_modified_after.max(tlog_modified_before),
                report_generated_at,
            };
            write_reports_meta(&reports_meta_path(&reports_root), &meta)?;
            break;
        }
        Ok(())
    })();
    if let Ok(mut guard) = RUN_GUARD.lock() {
        if let Some(entry) = guard.get_mut(&guard_key) {
            entry.in_flight = false;
            entry.last_run = Some(Instant::now());
        }
    }
    result?;
    Ok(RunOutcome::Ran(crate_root))
}

static WORKSPACE_GUARD: Lazy<Mutex<GuardEntry>> = Lazy::new(|| Mutex::new(GuardEntry::default()));

pub fn run_workspace_analysis(args: &serde_json::Value) -> Result<RunOutcome> {
    let reports_root = if let Some(root) = args.get("reports_root").and_then(|v| v.as_str()) {
        PathBuf::from(root)
    } else if let Ok(root) = std::env::var("CANON_REPORTS_OUT") {
        PathBuf::from(root)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("state")
            .join("reports_out")
    };
    let workspace_dir = reports_root.join("workspace");
    let tlog_path = canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG"));
    if let Ok(mut guard) = WORKSPACE_GUARD.lock() {
        if guard.in_flight {
            return Ok(RunOutcome::Skipped(workspace_dir));
        }
        guard.in_flight = true;
    }
    let result = (|| -> Result<()> {
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let cursor_before = current_tlog_cursor(&tlog_path)
                .ok_or_else(|| anyhow!("failed to read tlog cursor"))?;
            let tlog_modified_before = tlog_last_modified(&tlog_path).unwrap_or(0);
            invalidate_reports_if_stale(&reports_root, &tlog_path)?;
            crate::report_pipeline::generate_reports_from_tlog(&tlog_path, &workspace_dir)?;
            let crates = collect_crate_names_from_tlog(&tlog_path)?;
            for crate_name in crates {
                let crate_root = reports_root.join("crates").join(&crate_name);
                crate::report_pipeline::generate_reports_for_crate(&tlog_path, &crate_root, &crate_name)?;
            }
            crate::workspace::aggregator::aggregate_workspace(&reports_root)?;
            let report_generated_at = unix_timestamp();
            let cursor_after = current_tlog_cursor(&tlog_path)
                .ok_or_else(|| anyhow!("failed to read tlog cursor"))?;
            let tlog_modified_after = tlog_last_modified(&tlog_path).unwrap_or(0);
            if cursor_before != cursor_after || tlog_modified_after > report_generated_at {
                if attempts < 2 {
                    continue;
                }
                return Err(anyhow!("tlog changed during report generation"));
            }
            let meta = ReportsMeta {
                tlog_cursor: cursor_after,
                tlog_last_modified: tlog_modified_after.max(tlog_modified_before),
                report_generated_at,
            };
            write_reports_meta(&reports_meta_path(&reports_root), &meta)?;
            break;
        }
        Ok(())
    })();
    if let Ok(mut guard) = WORKSPACE_GUARD.lock() {
        guard.in_flight = false;
        guard.last_run = Some(Instant::now());
    }
    result?;
    Ok(RunOutcome::Ran(workspace_dir))
}

pub fn ensure_workspace_root(args: &serde_json::Value) -> Result<PathBuf> {
    if let Some(path) = args.get("workspace").and_then(|v| v.as_str()) {
        return Ok(PathBuf::from(path));
    }
    Ok(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

use canon_types::{EventDelta, EventMask, KernelEvent, KernelEventConsumer, KernelState};
use canon_event_log::{info, error as log_error};
use std::path::PathBuf;

use crate::report_pipeline::generate_reports_from_tlog;

#[derive(Debug, Default)]
pub struct ReportEventConsumer {
    pub last_tick: u64,
    pub event_count: usize,
    last_generated_tick: Option<u64>,
    in_flight: bool,
    tlog_path: Option<PathBuf>,
    out_root: Option<PathBuf>,
}
impl ReportEventConsumer {
    pub fn new() -> Self {
        let tlog_path = std::env::var("CANON_REPORTS_TLOG").ok().map(PathBuf::from);
        let out_root = std::env::var("CANON_REPORTS_OUT").ok().map(PathBuf::from);
        Self {
            last_tick: 0,
            event_count: 0,
            last_generated_tick: None,
            in_flight: false,
            tlog_path,
            out_root,
        }
    }
}

impl KernelEventConsumer for ReportEventConsumer {
    fn mask(&self) -> EventMask {
        EventMask::COMPILATION_UNIT_FINISHED
    }

    fn on_event(&mut self, delta: &EventDelta, _state: &KernelState) {
        if !matches!(delta.event, KernelEvent::CompilationUnitFinished { .. }) {
            return;
        }
        self.last_tick = delta.tick;
        self.event_count = self.event_count.saturating_add(1);
        let Some(tlog_path) = self.tlog_path.as_ref() else {
            log_error(
                "report_consumer",
                "missing_env",
                serde_json::json!({ "var": "CANON_REPORTS_TLOG" }),
            );
            return;
        };
        let out_dir = match resolve_out_dir(&delta.event, self.out_root.as_ref()) {
            Some(dir) => dir,
            None => {
                log_error(
                    "report_consumer",
                    "missing_env",
                    serde_json::json!({ "var": "CANON_REPORTS_OUT" }),
                );
                return;
            }
        };
        if self.in_flight {
            return;
        }
        self.in_flight = true;
        info(
            "report_consumer",
            "generate_reports_start",
            serde_json::json!({
                "tick": delta.tick,
                "tlog": tlog_path.display().to_string(),
                "out": out_dir.display().to_string()
            }),
        );
        if let Err(err) = generate_reports_from_tlog(tlog_path, &out_dir) {
            log_error(
                "report_consumer",
                "generate_reports_failed",
                serde_json::json!({
                    "error": err.to_string(),
                    "tick": delta.tick
                }),
            );
            self.in_flight = false;
            return;
        }
        info(
            "report_consumer",
            "generate_reports_done",
            serde_json::json!({
                "tick": delta.tick,
                "out": out_dir.display().to_string()
            }),
        );
        self.last_generated_tick = Some(delta.tick);
        self.in_flight = false;
    }
}

fn resolve_out_dir(event: &KernelEvent, out_root: Option<&PathBuf>) -> Option<PathBuf> {
    let root = match out_root {
        Some(path) => path.clone(),
        None => return None,
    };
    // If a legacy kernel dir is passed, normalize to its parent reports_out.
    if root
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s == "kernel")
        .unwrap_or(false)
    {
        if let Some(parent) = root.parent() {
            return resolve_out_dir(event, Some(&parent.to_path_buf()));
        }
    }
    let crate_name = match event {
        KernelEvent::CompilationUnitFinished { crate_name } => crate_name.as_str(),
        _ => "unknown",
    };
    let crate_dir = sanitize_crate_name(crate_name);
    if root.ends_with("crates") {
        return Some(root.join(crate_dir));
    }
    if root
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        == Some("crates")
    {
        return Some(root);
    }
    if root.join("crates").exists() || root.ends_with("reports_out") {
        return Some(root.join("crates").join(crate_dir));
    }
    if root.file_name().and_then(|s| s.to_str()) == Some("kernel") {
        return Some(root);
    }
    Some(root.join("crates").join(crate_dir))
}

fn sanitize_crate_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

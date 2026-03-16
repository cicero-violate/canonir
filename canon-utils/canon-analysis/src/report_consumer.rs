use canon_event::emit_debug::{info, error as log_error};
use canon_event::{CapabilityRequested, EventMask, RustcEventConsumer};
use canon_types::{EventDelta, RustcEvent, RustcState};
use std::path::PathBuf;

use crate::verify_reports_layout;
use canon_types::ReportLayout;

#[derive(Debug, Default)]
pub struct ReportEventConsumer {
    pub last_tick: u64,
    pub event_count: usize,
    last_generated_tick: Option<u64>,
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
            tlog_path,
            out_root,
        }
    }
}

impl RustcEventConsumer for ReportEventConsumer {
    fn mask(&self) -> EventMask {
        EventMask::COMPILATION_UNIT_FINISHED
    }

    fn on_event(&mut self, delta: &EventDelta, _state: &RustcState) {
        if !matches!(delta.event, RustcEvent::CompilationUnitFinished { .. }) {
            return;
        }
        self.last_tick = delta.tick;
        self.event_count = self.event_count.saturating_add(1);
        let crate_name = match &delta.event {
            RustcEvent::CompilationUnitFinished { crate_name } => crate_name.as_str(),
            _ => "unknown",
        };
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
        let batch_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string();
        let reports_root = resolve_reports_root(&out_dir);
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let request = CapabilityRequested {
            request_id: format!("analysis-{}-analysis.run", crate_name),
            name: "analysis.run".to_string(),
            args: serde_json::json!({
                "crate": crate_name,
                "batch_id": batch_id,
                "workspace": workspace.display().to_string(),
                "reports_root": reports_root.display().to_string()
            }),
        };
        let payload = serde_json::to_value(&request)
            .unwrap_or_else(|_| serde_json::json!({}));
        if let Err(err) = crate::capabilities::events::emit_analysis_event(
            tlog_path,
            "capability_requested",
            payload,
        ) {
            log_error(
                "report_consumer",
                "emit_capability_failed",
                serde_json::json!({
                    "error": err.to_string(),
                    "tick": delta.tick
                }),
            );
            return;
        }
        info(
            "report_consumer",
            "capability_requested",
            serde_json::json!({
                "tick": delta.tick,
                "tlog": tlog_path.display().to_string(),
                "out": out_dir.display().to_string()
            }),
        );
        if std::env::var("CANON_REPORTS_VERIFY_LAYOUT").ok().as_deref() == Some("1") {
            let layout = ReportLayout::from_crate_root(out_dir.clone());
            match verify_reports_layout(layout.root()) {
                Ok(_) => info(
                    "report_consumer",
                    "verify_layout_ok",
                    serde_json::json!({ "root": layout.root().display().to_string() }),
                ),
                Err(err) => log_error(
                    "report_consumer",
                    "verify_layout_failed",
                    serde_json::json!({ "error": err.to_string(), "root": layout.root().display().to_string() }),
                ),
            }
        }
        self.last_generated_tick = Some(delta.tick);
    }
}

fn resolve_out_dir(event: &RustcEvent, out_root: Option<&PathBuf>) -> Option<PathBuf> {
    let root = match out_root {
        Some(path) => path.clone(),
        None => return None,
    };
    // If a legacy kernel dir is passed, normalize to its parent reports_out.
    if root
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s == "rustc")
        .unwrap_or(false)
    {
        if let Some(parent) = root.parent() {
            return resolve_out_dir(event, Some(&parent.to_path_buf()));
        }
    }
    let crate_name = match event {
        RustcEvent::CompilationUnitFinished { crate_name } => crate_name.as_str(),
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
    if root.file_name().and_then(|s| s.to_str()) == Some("rustc") {
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

fn resolve_reports_root(out_dir: &PathBuf) -> PathBuf {
    let mut cur = out_dir.as_path();
    while let Some(name) = cur.file_name().and_then(|s| s.to_str()) {
        if name == "crates" {
            return cur.parent().unwrap_or(cur).to_path_buf();
        }
        cur = match cur.parent() {
            Some(parent) => parent,
            None => break,
        };
    }
    out_dir.clone()
}

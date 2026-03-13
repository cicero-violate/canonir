use canon_tlog_writer::{append_event_json, BinarySegmentWriter, CanonEvent};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub fn default_tlog_path() -> PathBuf {
    if let Ok(path) = std::env::var("CANON_TLOG_PATH") {
        return PathBuf::from(path);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon"));
    let binary_dir = cwd.join("state/kernel_logs/kernel.tlog.d");
    if binary_dir.exists() {
        return binary_dir;
    }
    let candidate = cwd.join("state/kernel_logs/kernel.tlog");
    if candidate.exists() {
        candidate
    } else {
        PathBuf::from("/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog")
    }
}

pub fn emit(kind: &str, payload: Value) {
    emit_human_log(kind, &payload);
    let event = SupervisorEvent::Generic {
        kind: kind.to_string(),
        payload,
    };
    let payload = match serde_json::to_value(&event) {
        Ok(v) => v,
        Err(_) => return,
    };
    if let Some(writer) = BINARY_WRITER.as_ref() {
        let event = CanonEvent::new("canon-supervisor", "supervisor_event", payload.clone());
        let _ = writer.append_event(&event);
        if !dual_write_enabled() {
            return;
        }
    }
    let path = default_tlog_path();
    let _ = append_event_json(&path, "canon-supervisor", "supervisor_event", payload);
}

fn emit_human_log(kind: &str, payload: &Value) {
    match kind {
        "process_spawned" => {
            let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            let pid = payload.get("pid").and_then(|v| v.as_i64()).unwrap_or(-1);
            println!("[SUPERVISOR] spawned {name} (pid={pid})");
        }
        "process_restarted" => {
            let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            let strategy = payload.get("strategy").and_then(|v| v.as_str()).unwrap_or("unknown");
            println!("[SUPERVISOR] restarting {name} ({strategy})");
        }
        "process_exit" => {
            let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            let reason = payload.get("reason").and_then(|v| v.as_str()).unwrap_or("exit");
            println!("[SUPERVISOR] {name} exited ({reason})");
        }
        "file_change_detected" => {
            let path = payload.get("path").and_then(|v| v.as_str()).unwrap_or("unknown");
            let krate = payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
            println!("[SUPERVISOR] file change in {krate}: {path}");
        }
        "build.started" => {
            let krate = payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
            println!("[SUPERVISOR] build started: {krate}");
        }
        "build.completed" => {
            let krate = payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
            let ok = payload.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            if ok {
                println!("[SUPERVISOR] build completed: {krate}");
            } else {
                println!("[SUPERVISOR] build failed: {krate}");
            }
        }
        "workspace.changed" => {
            let krate = payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
            println!("[SUPERVISOR] workspace changed: {krate}");
        }
        _ => {}
    }
}

fn binary_dir_from_path(path: &PathBuf) -> PathBuf {
    if path.is_dir() {
        return path.clone();
    }
    path.with_extension("tlog.d")
}

fn tlog_format_is_binary() -> bool {
    match std::env::var("CANON_TLOG_FORMAT") {
        Ok(format) => format.to_lowercase() != "jsonl",
        Err(_) => true,
    }
}

static BINARY_WRITER: Lazy<Option<BinarySegmentWriter>> = Lazy::new(|| {
    if tlog_format_is_binary() {
        let path = default_tlog_path();
        let dir = binary_dir_from_path(&path);
        BinarySegmentWriter::open(&dir).ok()
    } else {
        None
    }
});

fn dual_write_enabled() -> bool {
    matches!(std::env::var("CANON_TLOG_DUAL_WRITE").as_deref(), Ok("1"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupervisorEvent {
    Generic { kind: String, payload: Value },
}

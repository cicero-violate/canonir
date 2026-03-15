use canon_tlog_writer::{BinarySegmentWriter, CanonEvent, TlogWriter};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

fn default_tlog_path() -> PathBuf {
    if let Ok(path) = std::env::var("CANON_TLOG_PATH") {
        return PathBuf::from(path);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon"));
    let candidate = cwd.join("state/kernel_logs/kernel.tlog");
    if candidate.exists() {
        candidate
    } else {
        PathBuf::from("/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog")
    }
}

static JSONL_WRITER: Lazy<Option<TlogWriter>> = Lazy::new(|| {
    let path = default_tlog_path();
    match TlogWriter::open(&path) {
        Ok(writer) => Some(writer),
        Err(err) => {
            eprintln!("[tlog] failed to open {}: {}", path.display(), err);
            None
        }
    }
});

fn dual_write_enabled() -> bool {
    matches!(std::env::var("CANON_TLOG_DUAL_WRITE").as_deref(), Ok("1"))
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

pub fn emit(kind: &str, payload: Value) {
    let event = AgentEvent::Generic {
        kind: kind.to_string(),
        payload,
    };
    let payload = match serde_json::to_value(&event) {
        Ok(v) => v,
        Err(_) => return,
    };
    if let Some(writer) = BINARY_WRITER.as_ref() {
        let event = CanonEvent::new("canon-agent-v2", "agent_event", payload.clone());
        let _ = writer.append_event(&event);
        if !dual_write_enabled() {
            return;
        }
    }
    if let Some(writer) = JSONL_WRITER.as_ref() {
        let event = CanonEvent::new("canon-agent-v2", "agent_event", payload);
        let _ = writer.append_event(&event);
    }
}

fn binary_dir_from_path(path: &PathBuf) -> PathBuf {
    if path.is_dir() {
        return path.clone();
    }
    path.with_extension("tlog.d")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentEvent {
    Generic { kind: String, payload: Value },
}

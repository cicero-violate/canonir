use canon_event::{canon_emit, CanonEvent, EventConsumer, EventEmitterHandle, EventFilter, RustcEvent};
use serde_json::json;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub struct ErrorLogger {
    tlog_path: PathBuf,
    jsonl_path: PathBuf,
}

impl ErrorLogger {
    pub fn new(_path: Option<PathBuf>) -> Self {
        let tlog_path = resolve_canonical_tlog_path();
        let jsonl_path = resolve_error_jsonl_path();
        let _ = create_dir_all(&tlog_path);
        if let Some(parent) = jsonl_path.parent() {
            let _ = create_dir_all(parent);
        }
        Self { tlog_path, jsonl_path }
    }
}

impl EventConsumer for ErrorLogger {
    fn filter(&self) -> EventFilter {
        EventFilter::ErrorOnly
    }

    fn on_event(&mut self, event: &CanonEvent) {
        let (source, payload) = match event_to_payload(event) {
            Some(record) => record,
            None => return,
        };
        let _ = canon_emit!(source, "error_occurred", payload.clone(), &self.tlog_path);
        let _ = append_error_jsonl(&self.jsonl_path, &payload);
    }

    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}
}

fn resolve_canonical_tlog_path() -> PathBuf {
    PathBuf::from("/workspace/ai_sandbox/canon/canon-utils/state/event_log/event.tlog.d")
}

fn resolve_error_jsonl_path() -> PathBuf {
    PathBuf::from("/workspace/ai_sandbox/canon/canon-utils/state/reports_out/workspace/errors.jsonl")
}

fn event_to_payload(event: &CanonEvent) -> Option<(String, serde_json::Value)> {
    match event {
        CanonEvent::ErrorOccurred(payload) => Some((
            payload.source.clone(),
            serde_json::to_value(payload).unwrap_or_else(|_| json!({})),
        )),
        CanonEvent::CapabilityFailed(payload) => Some((
            "event-runtime".to_string(),
            json!({
                "kind": "capability_failed",
                "source": "event-runtime",
                "message": payload.error,
                "severity": "error",
                "context": {
                    "request_id": payload.request_id,
                    "capability": payload.name,
                },
                "trace_id": null,
            }),
        )),
        CanonEvent::NodeFailed(payload) => Some((
            "agent-consumer".to_string(),
            json!({
                "kind": "node_failed",
                "source": "agent-consumer",
                "message": payload.error.as_deref().unwrap_or("node_failed"),
                "severity": "error",
                "context": {
                    "node_id": payload.node_id,
                    "capability": payload.capability,
                    "request_id": payload.request_id,
                },
                "trace_id": null,
            }),
        )),
        CanonEvent::Code(code) => match &code.delta.event {
            RustcEvent::PanicCaptured(payload) => Some((
                "rustc".to_string(),
                json!({
                    "kind": "panic_captured",
                    "source": "rustc",
                    "message": payload.message,
                    "severity": "error",
                    "context": {
                        "def_id": payload.def_id,
                        "mir_variant": payload.mir_variant,
                        "lowering_stage": payload.lowering_stage,
                        "file": payload.file,
                        "span": payload.span,
                    },
                    "trace_id": null,
                }),
            )),
            RustcEvent::InvariantViolation(payload) => Some((
                "rustc".to_string(),
                json!({
                    "kind": "invariant_violation",
                    "source": "rustc",
                    "message": payload.message,
                    "severity": "error",
                    "context": {},
                    "trace_id": null,
                }),
            )),
            _ => None,
        },
        _ => None,
    }
}

fn append_error_jsonl(path: &PathBuf, payload: &serde_json::Value) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    writeln!(file, "{line}")
}

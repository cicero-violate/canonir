use canon_event::{canon_emit, new_error_occurred, CanonEvent, EventConsumer, EventEmitterHandle, EventFilter, RustcEvent};
use serde_json::json;
use std::collections::HashMap;
use std::fs::{create_dir_all, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub struct ErrorLogger {
    tlog_path: PathBuf,
    jsonl_path: PathBuf,
    seen: HashMap<u64, Instant>,
}

impl ErrorLogger {
    pub fn new(_path: Option<PathBuf>) -> Self {
        let tlog_path = resolve_canonical_tlog_path();
        let jsonl_path = resolve_error_jsonl_path();
        let _ = create_dir_all(&tlog_path);
        if let Some(parent) = jsonl_path.parent() {
            let _ = create_dir_all(parent);
        }
        Self { tlog_path, jsonl_path, seen: HashMap::new() }
    }

    fn should_emit(&mut self, source: &str, message: &str) -> bool {
        let key = dedup_key(source, message);
        let now = Instant::now();
        if self.seen.get(&key).is_some_and(|t| now.duration_since(*t) < Duration::from_secs(30)) {
            return false;
        }
        self.seen.insert(key, now);
        self.seen.retain(|_, t| now.duration_since(*t) < Duration::from_secs(30));
        true
    }
}

impl EventConsumer for ErrorLogger {
    fn filter(&self) -> EventFilter {
        EventFilter::ErrorOnly
    }

    fn on_event(&mut self, event: &CanonEvent) {
        if let CanonEvent::ErrorOccurred(payload) = event {
            let value = serde_json::to_value(payload).unwrap_or_else(|_| json!({}));
            let _ = append_error_jsonl(&self.jsonl_path, &value);
            return;
        }
        let (source, payload) = match event_to_payload(event) {
            Some(record) => record,
            None => return,
        };
        let message = payload.get("message").and_then(|v| v.as_str()).unwrap_or("");
        if !self.should_emit(&source, message) {
            return;
        }
        let _ = canon_emit!(source, "error_occurred", payload.clone(), &self.tlog_path);
        let _ = append_error_jsonl(&self.jsonl_path, &payload);
    }

    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}
}

fn dedup_key(source: &str, message: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    message.hash(&mut hasher);
    hasher.finish()
}

fn resolve_canonical_tlog_path() -> PathBuf {
    PathBuf::from("/workspace/ai_sandbox/canon/canon-utils/state/event_log/event.tlog.d")
}

fn resolve_error_jsonl_path() -> PathBuf {
    if let Ok(p) = std::env::var("CANON_REPORTS_OUT") {
        return PathBuf::from(p).join("workspace").join("errors.jsonl");
    }
    PathBuf::from("/workspace/ai_sandbox/canon/state/reports_out/workspace/errors.jsonl")
}

fn event_to_payload(event: &CanonEvent) -> Option<(String, serde_json::Value)> {
    match event {
        CanonEvent::CapabilityFailed(payload) => Some((
            "event-runtime".to_string(),
            serde_json::to_value(new_error_occurred(
                "capability_failed",
                "event-runtime",
                payload.error.clone(),
                "error",
                json!({
                    "request_id": payload.request_id.clone(),
                    "capability": payload.name.clone(),
                }),
                Some(payload.request_id.clone()),
            ))
            .unwrap_or_else(|_| json!({})),
        )),
        CanonEvent::NodeFailed(payload) => Some((
            "agent-consumer".to_string(),
            serde_json::to_value(new_error_occurred(
                "node_failed",
                "agent-consumer",
                payload.error.as_deref().unwrap_or("node_failed"),
                "error",
                json!({
                    "node_id": payload.node_id.clone(),
                    "capability": payload.capability.clone(),
                    "request_id": payload.request_id.clone(),
                }),
                if payload.request_id.is_empty() { None } else { Some(payload.request_id.clone()) },
            ))
            .unwrap_or_else(|_| json!({})),
        )),
        CanonEvent::LoopActed(payload) if !payload.success && payload.stderr != "skipped:batch_aborted" => Some((
            "act".to_string(),
            serde_json::to_value(new_error_occurred(
                "loop_acted_failed",
                "act",
                payload.stderr.clone(),
                "error",
                json!({
                    "tick": payload.tick,
                    "action_kind": payload.action_kind.clone(),
                    "capability_request_id": payload.capability_request_id.clone(),
                    "exit_code": payload.exit_code,
                }),
                payload.trace_id.clone(),
            ))
            .unwrap_or_else(|_| json!({})),
        )),
        CanonEvent::LoopVerified(payload) if !payload.passed => Some((
            "verify".to_string(),
            serde_json::to_value(new_error_occurred(
                "loop_verified_failed",
                "verify",
                payload.diagnostics.join("; "),
                "error",
                json!({
                    "tick": payload.tick,
                    "compiler_clean": payload.compiler_clean,
                    "tlog_clean": payload.tlog_clean,
                    "error_count": payload.error_count,
                }),
                payload.trace_id.clone(),
            ))
            .unwrap_or_else(|_| json!({})),
        )),
        CanonEvent::LoopRewarded(payload) if payload.halt => Some((
            "reward".to_string(),
            serde_json::to_value(new_error_occurred(
                "loop_rewarded_halt",
                "reward",
                "stagnant:halt",
                "error",
                json!({
                    "tick": payload.tick,
                    "reward": payload.reward,
                    "errors_before": payload.errors_before,
                    "errors_after": payload.errors_after,
                    "stagnant_ticks": payload.stagnant_ticks,
                }),
                payload.trace_id.clone(),
            ))
            .unwrap_or_else(|_| json!({})),
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
                    "error_id": Uuid::new_v4().to_string(),
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
                    "error_id": Uuid::new_v4().to_string(),
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

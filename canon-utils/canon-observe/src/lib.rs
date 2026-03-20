use canon_event::{CanonEvent, ErrorOccurred, EventConsumer, EventEmitterHandle, EventFilter, LoopObserved, LoopVerified, Tick};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct ObserveConsumer {
    tlog_path: PathBuf,
    emitter: Option<EventEmitterHandle>,
    goal_text: Option<String>,
    recent_compiler_errors: Vec<Value>,
    error_count: usize,
    warning_count: usize,
}

impl ObserveConsumer {
    pub fn new(_workspace: PathBuf, tlog_path: PathBuf) -> Self {
        Self { tlog_path, emitter: None, goal_text: None, recent_compiler_errors: Vec::new(), error_count: 0, warning_count: 0 }
    }
}

impl EventConsumer for ObserveConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn on_event(&mut self, event: &CanonEvent) {
        if let CanonEvent::ErrorOccurred(err) = event {
            self.capture_compiler_signal(err);
            return;
        }
        if let CanonEvent::LoopVerified(LoopVerified { passed, .. }) = event {
            if *passed {
                self.error_count = 0;
                self.warning_count = 0;
                self.recent_compiler_errors.clear();
            }
            return;
        }
        if let CanonEvent::PromptLoaded(prompt) = event {
            let is_goal = prompt.payload.get("prompt_id").and_then(|v| v.as_str()).map(|id| id == "AGENT_GOAL").unwrap_or(false)
                || prompt.payload.get("path").and_then(|v| v.as_str()).map(|path| path.contains("AGENT_GOAL")).unwrap_or(false);
            if is_goal {
                self.goal_text = prompt.payload.get("content").and_then(|v| v.as_str()).map(|s| s.to_string());
            }
            return;
        }
        let CanonEvent::Tick(Tick { tick }) = event else {
            return;
        };

        // Goal not yet set via PromptLoaded bus event — bootstrap events are skipped
        // on startup. Scan the tlog once to recover the goal from history.
        if self.goal_text.is_none() {
            self.goal_text = scan_tlog_for_goal(self.tlog_path.as_path());
        }

        let payload =
            LoopObserved { tick: *tick, error_count: self.error_count, warning_count: self.warning_count, compiler_errors: self.recent_compiler_errors.clone(), goal_text: self.goal_text.clone() };

        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::LoopObserved(payload));
        }
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }
}

impl ObserveConsumer {
    fn capture_compiler_signal(&mut self, err: &ErrorOccurred) {
        if err.source != "rustc" && err.source != "verify" {
            return;
        }
        if err.severity == "warning" {
            self.warning_count = self.warning_count.saturating_add(1);
        } else {
            self.error_count = self.error_count.saturating_add(1);
        }
        self.recent_compiler_errors.push(serde_json::json!({
            "reason": "error_occurred",
            "message": {
                "level": err.severity,
                "message": err.message,
            }
        }));
        if self.recent_compiler_errors.len() > 16 {
            let drop_n = self.recent_compiler_errors.len() - 16;
            self.recent_compiler_errors.drain(0..drop_n);
        }
    }
}

/// Scan all tlog segments (oldest first) for the latest `prompt_loaded` event
/// with path containing "AGENT_GOAL". Returns the content if found.
/// Only called once per ObserveConsumer lifetime (when goal_text is still None).
fn scan_tlog_for_goal(tlog_path: &Path) -> Option<String> {
    let dir = if tlog_path.is_dir() { tlog_path.to_path_buf() } else { tlog_path.with_extension("tlog.d") };
    let mut logs: Vec<PathBuf> = std::fs::read_dir(&dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().and_then(|s| s.to_str()) == Some("log")).collect();
    logs.sort();

    let mut found: Option<String> = None;
    for log_path in &logs {
        let content = match std::fs::read_to_string(log_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if v.get("kind").and_then(|k| k.as_str()) != Some("prompt_loaded") {
                continue;
            }
            let payload = v.get("payload").unwrap_or(&Value::Null);
            let is_goal = payload.get("path").and_then(|p| p.as_str()).map(|p| p.contains("AGENT_GOAL")).unwrap_or(false)
                || payload.get("prompt_id").and_then(|p| p.as_str()).map(|p| p == "AGENT_GOAL").unwrap_or(false);
            if is_goal {
                if let Some(c) = payload.get("content").and_then(|c| c.as_str()) {
                    found = Some(c.to_string()); // keep scanning for a later version
                }
            }
        }
    }
    found
}

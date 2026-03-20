use canon_event::{CanonEvent, EventConsumer, EventEmitterHandle, EventFilter, LoopObserved, Tick};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct ObserveConsumer {
    workspace: PathBuf,
    tlog_path: PathBuf,
    emitter: Option<EventEmitterHandle>,
    goal_text: Option<String>,
}

impl ObserveConsumer {
    pub fn new(workspace: PathBuf, tlog_path: PathBuf) -> Self {
        Self {
            workspace,
            tlog_path,
            emitter: None,
            goal_text: None,
        }
    }
}

impl EventConsumer for ObserveConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn on_event(&mut self, event: &CanonEvent) {
        if let CanonEvent::PromptLoaded(prompt) = event {
            let is_goal = prompt
                .payload
                .get("prompt_id")
                .and_then(|v| v.as_str())
                .map(|id| id == "AGENT_GOAL")
                .unwrap_or(false)
                || prompt
                    .payload
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|path| path.contains("AGENT_GOAL"))
                    .unwrap_or(false);
            if is_goal {
                self.goal_text = prompt
                    .payload
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
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

        let output = run_cargo_check(&self.workspace, Duration::from_secs(30));
        let (compiler_errors, error_count, warning_count) = parse_compiler_messages(&output.stdout, output.exit_code, output.timed_out);

        let payload = LoopObserved {
            tick: *tick,
            error_count,
            warning_count,
            compiler_errors,
            goal_text: self.goal_text.clone(),
        };

        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::LoopObserved(payload));
        }
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }
}

struct CommandOutput {
    stdout: String,
    exit_code: Option<i32>,
    timed_out: bool,
}

fn run_cargo_check(workspace: &Path, timeout: Duration) -> CommandOutput {
    let mut cmd = Command::new("cargo");
    cmd.arg("check")
        .arg("--message-format=json")
        .current_dir(workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let start = Instant::now();
    let Ok(mut child) = cmd.spawn() else {
        return CommandOutput {
            stdout: String::new(),
            exit_code: None,
            timed_out: false,
        };
    };

    let stdout = child.stdout.take();
    let (stdout_tx, stdout_rx) = mpsc::channel();

    if let Some(stdout) = stdout {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let mut reader = std::io::BufReader::new(stdout);
            let _ = std::io::Read::read_to_string(&mut reader, &mut buf);
            let _ = stdout_tx.send(buf);
        });
    } else {
        let _ = stdout_tx.send(String::new());
    }

    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_rx.recv_timeout(Duration::from_millis(200)).unwrap_or_default();
                return CommandOutput {
                    stdout,
                    exit_code: status.code(),
                    timed_out: false,
                };
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
            Err(_) => {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let stdout = stdout_rx.recv_timeout(Duration::from_millis(200)).unwrap_or_default();
    CommandOutput {
        stdout,
        exit_code: None,
        timed_out,
    }
}

fn extract_compact_error(value: &serde_json::Value) -> serde_json::Value {
    let msg = value.get("message");
    serde_json::json!({
        "reason": value.get("reason"),
        "message": {
            "level": msg.and_then(|m| m.get("level")),
            "message": msg.and_then(|m| m.get("message")),
            "spans": msg.and_then(|m| m.get("spans"))
                .and_then(|s| s.as_array())
                .map(|spans| spans.iter().take(1).map(|sp| serde_json::json!({
                    "file_name": sp.get("file_name"),
                    "line_start": sp.get("line_start"),
                    "column_start": sp.get("column_start"),
                })).collect::<Vec<_>>())
                .unwrap_or_default(),
        }
    })
}

fn parse_compiler_messages(
    stdout: &str,
    exit_code: Option<i32>,
    timed_out: bool,
) -> (Vec<Value>, usize, usize) {
    let mut compiler_errors = Vec::new();
    let mut error_count = 0usize;
    let mut warning_count = 0usize;

    for line in stdout.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            if value.get("reason").and_then(|v| v.as_str()) != Some("compiler-message") {
                continue;
            }
            if let Some(message) = value.get("message") {
                if let Some(level) = message.get("level").and_then(|v| v.as_str()) {
                    if level == "error" {
                        error_count += 1;
                    } else if level == "warning" {
                        warning_count += 1;
                    }
                }
            }
            compiler_errors.push(extract_compact_error(&value));
        }
    }

    if timed_out {
        error_count = error_count.max(1);
    } else if exit_code.unwrap_or(0) != 0 && error_count == 0 {
        error_count = 1;
    }

    (compiler_errors, error_count, warning_count)
}


/// Scan all tlog segments (oldest first) for the latest `prompt_loaded` event
/// with path containing "AGENT_GOAL". Returns the content if found.
/// Only called once per ObserveConsumer lifetime (when goal_text is still None).
fn scan_tlog_for_goal(tlog_path: &Path) -> Option<String> {
    let dir = if tlog_path.is_dir() {
        tlog_path.to_path_buf()
    } else {
        tlog_path.with_extension("tlog.d")
    };
    let mut logs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("log"))
        .collect();
    logs.sort();

    let mut found: Option<String> = None;
    for log_path in &logs {
        let content = match std::fs::read_to_string(log_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue; };
            if v.get("kind").and_then(|k| k.as_str()) != Some("prompt_loaded") {
                continue;
            }
            let payload = v.get("payload").unwrap_or(&Value::Null);
            let is_goal = payload
                .get("path")
                .and_then(|p| p.as_str())
                .map(|p| p.contains("AGENT_GOAL"))
                .unwrap_or(false)
                || payload
                    .get("prompt_id")
                    .and_then(|p| p.as_str())
                    .map(|p| p == "AGENT_GOAL")
                    .unwrap_or(false);
            if is_goal {
                if let Some(c) = payload.get("content").and_then(|c| c.as_str()) {
                    found = Some(c.to_string()); // keep scanning for a later version
                }
            }
        }
    }
    found
}

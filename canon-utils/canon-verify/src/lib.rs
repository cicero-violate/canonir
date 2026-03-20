use canon_event::{canon_emit, CanonEvent, EventConsumer, EventEmitterHandle, EventFilter, LoopActed, LoopVerified};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct VerifyConsumer {
    workspace: PathBuf,
    tlog_path: PathBuf,
    emitter: Option<EventEmitterHandle>,
    last_trace_id: Option<String>,
    last_execution_id: Option<String>,
    last_act_span_id: Option<String>,
    last_acted: Option<LoopActed>,
    last_verified_action_key: Option<String>,
}

impl VerifyConsumer {
    pub fn new(workspace: PathBuf, tlog_path: PathBuf) -> Self {
        Self { workspace, tlog_path, emitter: None, last_trace_id: None, last_execution_id: None, last_act_span_id: None, last_acted: None, last_verified_action_key: None }
    }
}

impl EventConsumer for VerifyConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn on_event(&mut self, event: &CanonEvent) {
        match event {
            CanonEvent::LoopActed(acted) => {
                // Keep action context for route_selected=validate-driven verification.
                self.last_trace_id = acted.trace_id.clone();
                self.last_execution_id = acted.execution_id.clone();
                self.last_act_span_id = acted.span_id.clone();
                self.last_acted = Some(acted.clone());
            }
            CanonEvent::Debug(debug) if debug.kind == "route_selected" => {
                let lane = debug.payload.get("approved_route").or_else(|| debug.payload.get("lane")).and_then(|v| v.as_str()).unwrap_or("");
                if lane != "validate" {
                    return;
                }
                let Some(acted) = self.last_acted.clone() else {
                    return;
                };
                let action_key = acted_action_key(&acted);
                self.emit_debug(
                    "verify_scheduled",
                    serde_json::json!({
                        "tick": acted.tick,
                        "action_kind": acted.action_kind,
                        "action_key": action_key,
                        "source": "route_validate",
                    }),
                );
                if self.last_verified_action_key.as_deref() == Some(action_key.as_str()) {
                    self.emit_debug(
                        "verify_dedupe_skip",
                        serde_json::json!({
                            "tick": acted.tick,
                            "action_kind": acted.action_kind,
                            "action_key": action_key,
                            "source": "route_validate",
                        }),
                    );
                    return;
                }
                self.last_verified_action_key = Some(action_key);
                self.verify_acted(&acted);
            }
            _ => {}
        }
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
        self.emit_debug(
            "verify_consumer_started",
            serde_json::json!({
                "component": "canon-verify",
                "version": env!("CARGO_PKG_VERSION"),
                "build_hash": option_env!("CANON_COMMIT_ID").unwrap_or("unknown"),
            }),
        );
    }
}

impl VerifyConsumer {
    fn emit_debug(&self, kind: &str, payload: Value) {
        if let Some(emitter) = self.emitter.as_ref() {
            let _ = canon_emit!(emitter; "verify_consumer", kind, payload);
        }
    }

    fn verify_acted(&mut self, acted: &LoopActed) {
        let action_key = acted_action_key(acted);
        self.emit_debug(
            "verify_start",
            serde_json::json!({
                "tick": acted.tick,
                "action_kind": acted.action_kind,
                "action_key": action_key,
            }),
        );
        if acted.action_kind == "no_op" {
            let diagnostics: Vec<String> = Vec::new();
            self.emit_debug(
                "verify_result",
                serde_json::json!({
                    "tick": acted.tick,
                    "action_kind": acted.action_kind,
                    "action_key": action_key,
                    "passed": true,
                    "diagnostics_count": diagnostics.len(),
                    "diagnostics": diagnostics,
                    "source": "no_op_fast_path",
                }),
            );
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit(CanonEvent::LoopVerified(LoopVerified {
                    tick: acted.tick,
                    passed: true,
                    compiler_clean: true,
                    tlog_clean: true,
                    error_count: 0,
                    diagnostics: Vec::new(),
                    trace_id: self.last_trace_id.clone(),
                    execution_id: self.last_execution_id.clone(),
                    span_id: Some(uuid::Uuid::new_v4().to_string()),
                    parent_span_id: self.last_act_span_id.clone(),
                }));
            }
            return;
        }

        let output = run_cargo_check(&self.workspace, Duration::from_secs(30));
        let (_compiler_errors, error_count) = parse_compiler_messages(&output.stdout, output.exit_code, output.timed_out);
        let compiler_clean = error_count == 0;

        let tlog_tail = read_tlog_tail(&self.tlog_path, 10);
        let (tlog_clean, tlog_diag) = check_tlog_clean(&tlog_tail, acted.duration_ms);
        let (file_ok, file_diag) = check_file_written(&tlog_tail, acted);

        let mut diagnostics = Vec::new();
        if !compiler_clean {
            diagnostics.push("compiler_errors".to_string());
        }
        if let Some(diag) = tlog_diag {
            diagnostics.push(diag);
        }
        if let Some(diag) = file_diag {
            diagnostics.push(diag);
        }

        let passed = compiler_clean && tlog_clean && file_ok;
        self.emit_debug(
            "verify_result",
            serde_json::json!({
                "tick": acted.tick,
                "action_kind": acted.action_kind,
                "action_key": action_key,
                "passed": passed,
                "diagnostics_count": diagnostics.len(),
                "diagnostics": diagnostics.clone(),
            }),
        );
        let payload = LoopVerified {
            tick: acted.tick,
            passed,
            compiler_clean,
            tlog_clean,
            error_count,
            diagnostics,
            trace_id: self.last_trace_id.clone(),
            execution_id: self.last_execution_id.clone(),
            span_id: Some(uuid::Uuid::new_v4().to_string()),
            parent_span_id: self.last_act_span_id.clone(),
        };

        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::LoopVerified(payload));
        }
    }
}

fn acted_action_key(acted: &LoopActed) -> String {
    if !acted.capability_request_id.is_empty() {
        return acted.capability_request_id.clone();
    }
    format!("{}:{}", acted.tick, acted.action_kind)
}

struct CommandOutput {
    stdout: String,
    exit_code: Option<i32>,
    timed_out: bool,
}

fn run_cargo_check(workspace: &Path, timeout: Duration) -> CommandOutput {
    let mut cmd = Command::new("cargo");
    cmd.arg("check").arg("--message-format=json").current_dir(workspace).stdout(Stdio::piped()).stderr(Stdio::null());

    let start = Instant::now();
    let Ok(mut child) = cmd.spawn() else {
        return CommandOutput { stdout: String::new(), exit_code: None, timed_out: false };
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
                return CommandOutput { stdout, exit_code: status.code(), timed_out: false };
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
    CommandOutput { stdout, exit_code: None, timed_out }
}

fn parse_compiler_messages(stdout: &str, exit_code: Option<i32>, timed_out: bool) -> (Vec<Value>, usize) {
    let mut compiler_errors = Vec::new();
    let mut error_count = 0usize;

    for line in stdout.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            if value.get("reason").and_then(|v| v.as_str()) != Some("compiler-message") {
                continue;
            }
            if let Some(message) = value.get("message") {
                if let Some(level) = message.get("level").and_then(|v| v.as_str()) {
                    if level == "error" {
                        error_count += 1;
                    }
                }
            }
            compiler_errors.push(value);
        }
    }

    if timed_out {
        error_count = error_count.max(1);
    } else if exit_code.unwrap_or(0) != 0 && error_count == 0 {
        error_count = 1;
    }

    (compiler_errors, error_count)
}

fn read_tlog_tail(tlog_path: &Path, max_lines: usize) -> Vec<Value> {
    let Some(log_path) = latest_log_file(tlog_path) else {
        return Vec::new();
    };
    let content = std::fs::read_to_string(log_path).unwrap_or_default();
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].iter().filter_map(|line| serde_json::from_str::<Value>(line).ok()).collect()
}

fn latest_log_file(tlog_path: &Path) -> Option<PathBuf> {
    let dir = if tlog_path.is_dir() { tlog_path.to_path_buf() } else { tlog_path.with_extension("tlog.d") };
    let mut logs: Vec<PathBuf> =
        std::fs::read_dir(dir).ok()?.filter_map(|entry| entry.ok()).map(|entry| entry.path()).filter(|path| path.extension().and_then(|s| s.to_str()) == Some("log")).collect();
    logs.sort();
    logs.pop()
}

fn check_tlog_clean(tlog_tail: &[Value], duration_ms: u64) -> (bool, Option<String>) {
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_else(|_| Duration::from_secs(0)).as_millis() as u64;
    let start_ts = now_ms.saturating_sub(duration_ms);

    for entry in tlog_tail {
        let kind = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind != "error_occurred" {
            continue;
        }
        let ts = entry.get("ts").and_then(|v| v.as_u64()).unwrap_or(0);
        if ts >= start_ts {
            return (false, Some("tlog_error_occurred".to_string()));
        }
    }
    (true, None)
}

fn check_file_written(tlog_tail: &[Value], acted: &LoopActed) -> (bool, Option<String>) {
    if acted.action_kind != "patch_file" && acted.action_kind != "write_file" {
        return (true, None);
    }
    let mut path: Option<String> = None;
    for entry in tlog_tail {
        let kind = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind != "capability_requested" {
            continue;
        }
        let Some(payload) = entry.get("payload") else {
            continue;
        };
        let request_id = payload.get("request_id").and_then(|v| v.as_str());
        if request_id != Some(acted.capability_request_id.as_str()) {
            continue;
        }
        path = payload.get("args").and_then(|args| args.get("path")).and_then(|v| v.as_str()).map(|s| s.to_string());
        break;
    }
    let Some(path) = path else {
        return (false, Some("file_path_not_found".to_string()));
    };
    match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => (true, None),
        Ok(_) => (false, Some(format!("file_not_regular:{path}"))),
        Err(_) => (false, Some(format!("file_missing:{path}"))),
    }
}

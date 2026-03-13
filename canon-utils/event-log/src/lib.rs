use serde::Serialize;
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize)]
struct LogRecord<'a> {
    ts_ms: u128,
    level: &'a str,
    target: &'a str,
    message: &'a str,
    fields: Value,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn log_path() -> String {
    std::env::var("CANON_EVENT_RUNTIME_LOG")
        .unwrap_or_else(|_| "/workspace/ai_sandbox/canon/state/event_runtime.log".to_string())
}

pub fn log_event(target: &str, level: &str, message: &str, fields: Value) {
    let _guard = LOG_LOCK.lock();
    let path = log_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let record = LogRecord {
            ts_ms: now_ms(),
            level,
            target,
            message,
            fields,
        };
        if let Ok(line) = serde_json::to_string(&record) {
            let _ = writeln!(file, "{}", line);
        }
    }
}

pub fn info(target: &str, message: &str, fields: Value) {
    log_event(target, "info", message, fields);
}

pub fn warn(target: &str, message: &str, fields: Value) {
    log_event(target, "warn", message, fields);
}

pub fn error(target: &str, message: &str, fields: Value) {
    log_event(target, "error", message, fields);
}

use backtrace::Backtrace;
use serde::Serialize;
use serde_json::json;
use std::cell::RefCell;
use std::panic::PanicHookInfo;
use std::path::PathBuf;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Once, OnceLock};

use crate::event_stream::event::{PanicFrame, PanicSymbol};

#[derive(Serialize)]
struct PanicRecord {
    def_id: String,
    mir_variant: Option<String>,
    lowering_stage: Option<String>,
    file: Option<String>,
    span: Option<String>,
    message: String,
    frames: Vec<PanicFrame>,
}

struct PanicSnapshot {
    message: String,
    frames: Vec<PanicFrame>,
}

static PANIC_LOG_ROOT: OnceLock<PathBuf> = OnceLock::new();
static PANIC_HOOK_ONCE: Once = Once::new();
static PANIC_SEQ: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static LAST_PANIC: RefCell<Option<PanicSnapshot>> = RefCell::new(None);
}

pub fn panic_log_root() -> Option<PathBuf> {
    PANIC_LOG_ROOT.get().cloned()
}

pub fn set_panic_log_root(root: PathBuf) {
    let _ = PANIC_LOG_ROOT.set(root);
}

pub fn install_panic_hook() {
    PANIC_HOOK_ONCE.call_once(|| {
        std::panic::set_hook(Box::new(move |info| {
            let snapshot = PanicSnapshot {
                message: panic_message_from_info(info),
                frames: frames_from_backtrace(&Backtrace::new()),
            };
            LAST_PANIC.with(|slot| {
                *slot.borrow_mut() = Some(snapshot);
            });
            // Suppress default panic backtrace and any console output.
        }));
    });
}

pub fn append_panic_record(def_id: &str, message: &str) {
    let root = PANIC_LOG_ROOT
        .get()
        .expect("panic log root not configured")
        .clone();
    let logs_dir = root.join("state").join("event_log");
    std::fs::create_dir_all(&logs_dir).expect("panic log directory creation failed");
    let _path = logs_dir.join("event.tlog");
    let snapshot = take_last_panic_snapshot();
    let (message, mut frames) = if let Some(snapshot) = snapshot {
        (snapshot.message, snapshot.frames)
    } else {
        (
            message.to_string(),
            frames_from_backtrace(&Backtrace::new()),
        )
    };
    let seq = PANIC_SEQ.fetch_add(1, Ordering::Relaxed);
    if frames.is_empty() {
        frames.push(PanicFrame {
            frame_index: 0,
            symbols: Vec::new(),
        });
    }
    if let Some(frame) = frames.get_mut(0) {
        frame.symbols.insert(
            0,
            PanicSymbol {
                symbol: format!("panic#{}", seq),
                file: None,
                line: None,
            },
        );
    }
    let (mir_variant, lowering_stage, file, span) = extract_panic_tags(&message);
    let record = PanicRecord {
        def_id: def_id.to_string(),
        mir_variant,
        lowering_stage,
        file,
        span,
        message,
        frames,
    };
    let error_def_id = record.def_id.clone();
    let error_mir_variant = record.mir_variant.clone();
    let error_lowering_stage = record.lowering_stage.clone();
    let error_file = record.file.clone();
    let error_span = record.span.clone();
    let _ = write_mir_error_jsonl(
        &error_def_id,
        &record.message,
        &error_mir_variant,
        &error_lowering_stage,
        &error_file,
        &error_span,
    );
    return;
}

fn write_mir_error_jsonl(
    def_id: &str,
    message: &str,
    mir_variant: &Option<String>,
    lowering_stage: &Option<String>,
    file: &Option<String>,
    span: &Option<String>,
) -> std::io::Result<()> {
    let root = std::env::var("CANON_REPORTS_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/state/reports_out"));
    let path = root.join("mir_errors.jsonl");
    if let Some(parent) = path.parent() {
        let _ = create_dir_all(parent);
    }
    let mut fileh = OpenOptions::new().create(true).append(true).open(&path)?;
    let payload = json!({
        "def_id": def_id,
        "message": truncate_message(message),
        "mir_variant": mir_variant,
        "lowering_stage": lowering_stage,
        "file": file,
        "span": span,
    });
    let line = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    writeln!(fileh, "{line}")
}

const MAX_MSG: usize = 512;

fn truncate_message(msg: &str) -> String {
    if msg.len() <= MAX_MSG {
        msg.to_string()
    } else {
        msg[..MAX_MSG].to_string()
    }
}


fn extract_tag(message: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    for part in message.split_whitespace() {
        if let Some(rest) = part.strip_prefix(&needle) {
            return Some(rest.to_string());
        }
    }
    None
}

fn extract_panic_tags(
    message: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let mir_variant = extract_tag(message, "mir_variant");
    let lowering_stage = extract_tag(message, "lowering_stage");
    let file = extract_tag(message, "file");
    let span = extract_tag(message, "span");
    (mir_variant, lowering_stage, file, span)
}

fn take_last_panic_snapshot() -> Option<PanicSnapshot> {
    LAST_PANIC.with(|slot| slot.borrow_mut().take())
}

fn frames_from_backtrace(bt: &Backtrace) -> Vec<PanicFrame> {
    let mut frames = Vec::new();
    let mut frame_index = 0usize;
    for frame in bt.frames() {
        let symbols = frame
            .symbols()
            .iter()
            .map(|symbol| PanicSymbol {
                symbol: symbol
                    .name()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string()),
                file: symbol.filename().map(|p| p.display().to_string()),
                line: symbol.lineno(),
            })
            .collect();
        frames.push(PanicFrame {
            frame_index,
            symbols,
        });
        frame_index += 1;
    }
    frames
}

fn panic_message_from_info(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let base = if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<unknown>".to_string()
    };
    base
}

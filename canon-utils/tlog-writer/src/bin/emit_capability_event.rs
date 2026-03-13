use anyhow::{anyhow, Result};
use canon_tlog_writer::{append_event_json, BinarySegmentWriter, CanonEvent};
use canon_types::CapabilityRequested;
use std::env;
use std::path::{Path, PathBuf};

fn default_tlog_path() -> PathBuf {
    if let Ok(path) = env::var("CANON_TLOG_PATH") {
        return PathBuf::from(path);
    }
    PathBuf::from("/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog.d")
}

fn tlog_format_is_binary(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    match env::var("CANON_TLOG_FORMAT") {
        Ok(format) => format.to_lowercase() != "jsonl",
        Err(_) => true,
    }
}

fn generate_request_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let pid = std::process::id();
    format!("cap-{}-{}", pid, now)
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut tlog_path: Option<PathBuf> = None;
    let mut name: Option<String> = None;
    let mut args_json: Option<String> = None;
    let mut request_id: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--tlog" => {
                i += 1;
                tlog_path = args.get(i).map(PathBuf::from);
            }
            "--name" => {
                i += 1;
                name = args.get(i).cloned();
            }
            "--args" => {
                i += 1;
                args_json = args.get(i).cloned();
            }
            "--request-id" => {
                i += 1;
                request_id = args.get(i).cloned();
            }
            _ => {}
        }
        i += 1;
    }

    let tlog_path = tlog_path.unwrap_or_else(default_tlog_path);
    let name = name.ok_or_else(|| anyhow!("missing --name <capability>"))?;
    let args_json = args_json.unwrap_or_else(|| "{}".to_string());
    let args_value: serde_json::Value = serde_json::from_str(&args_json)?;
    let request_id = request_id.unwrap_or_else(generate_request_id);

    let request = CapabilityRequested {
        request_id,
        name,
        args: args_value,
    };

    let payload = serde_json::to_value(&request)?;
    let canon = CanonEvent::new("event-runtime", "capability_requested", payload);

    if tlog_format_is_binary(&tlog_path) {
        let dir = if tlog_path.is_dir() {
            tlog_path.clone()
        } else {
            tlog_path.with_extension("tlog.d")
        };
        let writer = BinarySegmentWriter::open(&dir)?;
        let _ = writer.append_event(&canon);
        return Ok(());
    }

    append_event_json(&tlog_path, "event-runtime", "capability_requested", serde_json::to_value(&request)?)?;
    Ok(())
}

use anyhow::{anyhow, Result};
use canon_event::canon_emit;
use std::env;
use std::path::PathBuf;

fn default_tlog_path() -> PathBuf {
    if let Ok(path) = env::var("CANON_TLOG_PATH") {
        return PathBuf::from(path);
    }
    PathBuf::from("/workspace/ai_sandbox/canon/state/event_log/event.tlog.d")
}

fn generate_request_id() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
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
            "--name" | "--capability" => {
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

    let payload = serde_json::json!({
        "request_id": request_id,
        "name": name,
        "args": args_value,
    });
    canon_emit!(root; "event-runtime", "capability_requested", payload, &tlog_path)
}

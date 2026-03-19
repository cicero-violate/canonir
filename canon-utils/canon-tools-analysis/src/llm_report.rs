use canon_event::{CapabilityCompleted, CapabilityFailed};
use canon_event_store::{read_any_events_from_path, AnyEvent, extract_capability_request};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Default)]
struct LlmRecord {
    request_id: String,
    prompt: Option<String>,
    role: Option<String>,
    raw: Option<bool>,
    endpoint: Option<String>,
    url: Option<String>,
    response: Option<Value>,
    error: Option<String>,
}

pub fn write_llm_reports_from_tlog(tlog_path: &Path, reports_root: &Path) -> anyhow::Result<()> {
    let mut by_id: BTreeMap<String, LlmRecord> = BTreeMap::new();
    let events = read_any_events_from_path(tlog_path)?;
    for event in events {
        let AnyEvent::Canon(canon) = event else { continue };

        if let Some(req) = extract_capability_request(&canon) {
            if req.name == "llm.call" {
                let entry = by_id.entry(req.request_id.clone()).or_insert_with(|| LlmRecord {
                    request_id: req.request_id.clone(),
                    ..Default::default()
                });
                if let Some(prompt) = req.args.get("prompt").and_then(|v| v.as_str()) {
                    entry.prompt = Some(prompt.to_string());
                }
                if let Some(role) = req.args.get("role").and_then(|v| v.as_str()) {
                    entry.role = Some(role.to_string());
                }
                if let Some(raw) = req.args.get("raw").and_then(|v| v.as_bool()) {
                    entry.raw = Some(raw);
                }
            }
        }

        match canon.kind.as_str() {
            "request_dispatch" if canon.source == "llm_executor" => {
                if let Some(id) = canon.payload.get("request_id").and_then(|v| v.as_str()) {
                    let entry = by_id.entry(id.to_string()).or_insert_with(|| LlmRecord {
                        request_id: id.to_string(),
                        ..Default::default()
                    });
                    entry.endpoint = canon.payload.get("endpoint").and_then(|v| v.as_str()).map(|s| s.to_string());
                    entry.url = canon.payload.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
                }
            }
            "capability_completed" => {
                if let Ok(done) = serde_json::from_value::<CapabilityCompleted>(canon.payload.clone()) {
                    if done.name == "llm.call" {
                        let entry = by_id.entry(done.request_id.clone()).or_insert_with(|| LlmRecord {
                            request_id: done.request_id.clone(),
                            ..Default::default()
                        });
                        if let Some(result) = done.result.get("result") {
                            entry.response = Some(result.clone());
                        }
                    }
                }
            }
            "capability_failed" => {
                if let Ok(failed) = serde_json::from_value::<CapabilityFailed>(canon.payload.clone()) {
                    if failed.name == "llm.call" {
                        let entry = by_id.entry(failed.request_id.clone()).or_insert_with(|| LlmRecord {
                            request_id: failed.request_id.clone(),
                            ..Default::default()
                        });
                        entry.error = Some(failed.error);
                    }
                }
            }
            _ => {}
        }
    }

    let out_dir = reports_root.join("llm").join("responses");
    std::fs::create_dir_all(&out_dir)?;
    for (id, rec) in by_id {
        let dir = out_dir.join(&id);
        std::fs::create_dir_all(&dir)?;
        let request = serde_json::json!({
            "request_id": rec.request_id,
            "prompt": rec.prompt,
            "role": rec.role,
            "raw": rec.raw,
            "endpoint": rec.endpoint,
            "url": rec.url,
        });
        let _ = std::fs::write(dir.join("request.json"), serde_json::to_string_pretty(&request)?);
        if let Some(resp) = rec.response {
            let body = serde_json::json!({
                "request_id": id,
                "response": resp,
            });
            let _ = std::fs::write(dir.join("response.json"), serde_json::to_string_pretty(&body)?);
        }
        if let Some(err) = rec.error {
            let body = serde_json::json!({
                "request_id": id,
                "error": err,
            });
            let _ = std::fs::write(dir.join("error.json"), serde_json::to_string_pretty(&body)?);
        }
    }
    Ok(())
}

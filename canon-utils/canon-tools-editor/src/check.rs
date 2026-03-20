use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

#[derive(serde::Serialize)]
pub struct CargoCheckJson {
    pub diagnostics: Vec<serde_json::Value>,
    pub success: bool,
}

pub fn run_cargo_check_json(project: &Path) -> Result<CargoCheckJson, Box<dyn std::error::Error>> {
    let mut cmd = Command::new("cargo");
    cmd.arg("check").arg("--message-format=json").current_dir(project);
    let output = cmd.output()?;
    let success = output.status.success();
    let mut diagnostics = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value.get("reason").and_then(|v| v.as_str()) == Some("compiler-message") {
                if let Some(message) = value.get("message") {
                    diagnostics.push(message.clone());
                }
                continue;
            }
            // analysis_capture may emit raw rustc JSON diagnostics (no "reason")
            if let Some(message) = value.get("message") {
                if message.is_object() {
                    diagnostics.push(message.clone());
                    continue;
                }
            }
            if value.get("level").is_some() && value.get("message").is_some() {
                diagnostics.push(value.clone());
            }
        }
    }
    if diagnostics.is_empty() && !success {
        diagnostics.push(json!({
            "level": "error",
            "code": { "code": "unknown" },
            "message": "cargo check failed without JSON diagnostics"
        }));
    }
    // keep diagnostics from this cargo check only (do not read analysis/errors.json)
    Ok(CargoCheckJson { diagnostics, success })
}

pub fn accumulate_error_counts_json(messages: &[serde_json::Value], counts: &mut BTreeMap<String, usize>) {
    for msg in messages {
        if msg.get("level").and_then(|v| v.as_str()) != Some("error") {
            continue;
        }
        if let Some(code) = msg.get("code").and_then(|c| c.get("code")).and_then(|c| c.as_str()) {
            *counts.entry(code.to_string()).or_default() += 1;
        }
    }
}

pub fn compute_delta_error_counts(baseline: &BTreeMap<String, usize>, after: &BTreeMap<String, usize>) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    for key in baseline.keys().chain(after.keys()) {
        let base = *baseline.get(key).unwrap_or(&0) as i64;
        let new = *after.get(key).unwrap_or(&0) as i64;
        let delta = new - base;
        if delta != 0 {
            out.insert(key.clone(), delta);
        }
    }
    out
}

pub fn summarize_error_categories(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut by_code: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for msg in messages {
        if msg.get("level").and_then(|v| v.as_str()) != Some("error") {
            continue;
        }
        let code = msg.get("code").and_then(|c| c.get("code")).and_then(|c| c.as_str()).unwrap_or("unknown").to_string();
        let desc = msg.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
        let entry = by_code.entry(code).or_insert((desc.clone(), 0));
        entry.1 += 1;
        if entry.0.is_empty() && !desc.is_empty() {
            entry.0 = desc;
        }
    }
    by_code
        .into_iter()
        .map(|(code, (description, count))| {
            serde_json::json!({
                "code": code,
                "description": description,
                "count": count
            })
        })
        .collect()
}

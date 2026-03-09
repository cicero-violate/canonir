use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

#[derive(serde::Serialize)]
pub struct CargoCheckJson {
    pub diagnostics: Vec<serde_json::Value>,
}

pub fn run_cargo_check_json(project: &Path) -> Result<CargoCheckJson, Box<dyn std::error::Error>> {
    let mut cmd = Command::new("cargo");
    cmd.arg("check").arg("--message-format=json").current_dir(project);
    let output = cmd.output()?;
    let mut diagnostics = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value.get("reason").and_then(|v| v.as_str()) == Some("compiler-message") {
                if let Some(message) = value.get("message") {
                    diagnostics.push(message.clone());
                }
            }
        }
    }
    if diagnostics.is_empty() && !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("error") {
                diagnostics.push(json!({ "level": "error", "message": trimmed }));
            }
        }
    }
    Ok(CargoCheckJson { diagnostics })
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

pub fn compute_delta_error_counts(
    baseline: &BTreeMap<String, usize>,
    after: &BTreeMap<String, usize>,
) -> BTreeMap<String, i64> {
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

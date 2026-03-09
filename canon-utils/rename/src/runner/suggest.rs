use super::config::SuggestConfig;
use super::symbols::{load_symbols_entries, write_symbols_entries};
use crate::core::rustc_session::RustcSession;
use crate::core::symbol_id::normalize_symbol_id;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub(crate) fn run_suggest_names(config: SuggestConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut entries = load_symbols_entries(&config.symbols_json)?;
    let session = Arc::new(RustcSession::build(&config.project)?);
    let mut pending: Vec<usize> = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        let rename_safe = entry.get("rename_safe").and_then(|v| v.as_bool()).unwrap_or(true);
        if !rename_safe {
            continue;
        }
        let symbol_id = entry.get("symbol_id").and_then(|v| v.as_str()).unwrap_or("");
        let tail = symbol_id.rsplit("::").next().unwrap_or(symbol_id);
        let new_name = entry.get("new_name").and_then(|v| v.as_str()).unwrap_or("");
        if new_name == tail {
            pending.push(idx);
        }
    }
    let groups = group_pending_by_file(&entries, &pending, &session);
    for (file, indices) in groups {
        let source = std::fs::read_to_string(&file).unwrap_or_default();
        for chunk in indices.chunks(config.batch_size) {
            let batch: Vec<&serde_json::Value> = chunk.iter().map(|&i| &entries[i]).collect();
            let prompt = build_prompt(&source, &batch);
            let response = call_llm_for_suggestions(&prompt, &config.model)?;
            let suggestions = parse_llm_response(&response, &batch);
            for (idx, suggestion) in chunk.iter().zip(suggestions.into_iter()) {
                if let Some(name) = suggestion {
                    if let Some(obj) = entries.get_mut(*idx) {
                        if !config.dry_run {
                            obj["new_name"] = serde_json::Value::String(name);
                        }
                    }
                }
            }
            if config.dry_run {
                continue;
            }
            write_symbols_entries(&config.symbols_json, &entries)?;
        }
    }
    Ok(())
}

fn group_pending_by_file(
    entries: &[serde_json::Value],
    pending: &[usize],
    session: &RustcSession,
) -> HashMap<PathBuf, Vec<usize>> {
    let mut groups: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    for &idx in pending {
        let symbol_id = entries[idx].get("symbol_id").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(file) = primary_file_for_symbol(session, symbol_id) {
            groups.entry(file).or_default().push(idx);
        }
    }
    groups
}

fn primary_file_for_symbol(session: &RustcSession, symbol_id: &str) -> Option<PathBuf> {
    let norm = normalize_symbol_id(symbol_id);
    let spans = session.spans_for(&norm)?;
    spans.keys().next().cloned()
}

fn build_prompt(source: &str, symbols: &[&serde_json::Value]) -> String {
    let symbol_list = symbols
        .iter()
        .map(|s| {
            let symbol_id = s.get("symbol_id").and_then(|v| v.as_str()).unwrap_or("");
            let kind = s.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown");
            let tail = symbol_id.rsplit("::").next().unwrap_or(symbol_id);
            format!("- symbol_id: {symbol_id}\n  kind: {kind}\n  current_name: {tail}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"You are a Rust refactoring assistant. Given source code and a list of symbols, suggest better names.

Rules:
- Output ONLY a JSON array, one object per symbol, in the same order as the input list.
- Each object: {{"symbol_id": "...", "new_name": "..."}}
- new_name must be a valid Rust identifier (snake_case for fn/const/static/module, CamelCase for struct/enum/trait/type).
- If the current name is already clear and appropriate, return the same name (no change).
- Do not rename symbols that look like they implement a pattern or protocol (e.g. run_tick, handle_message).
- Be conservative: only suggest a rename if the new name is clearly better.
- Do not add prefixes like "new_" or "my_".
- Names must be consistent with other symbols in the same module.

Source file:
```rust
{source}
```

Symbols to rename:
{symbol_list}

Respond with only the JSON array, no explanation."#
    )
}

fn call_llm_for_suggestions(prompt: &str, model: &str) -> Result<String, Box<dyn std::error::Error>> {
    let cmd = std::env::var("RENAME_SUGGEST_CMD")
        .map_err(|_| "RENAME_SUGGEST_CMD not set (path to LLM wrapper)")?;
    let mut parts = cmd.split_whitespace();
    let exe = parts.next().ok_or("RENAME_SUGGEST_CMD empty")?;
    let mut command = Command::new(exe);
    for arg in parts {
        command.arg(arg);
    }
    command.arg("--model").arg(model);
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(prompt.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_llm_response(response: &str, expected_symbols: &[&serde_json::Value]) -> Vec<Option<String>> {
    let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(response) else {
        eprintln!("LLM response was not valid JSON array, skipping batch");
        return vec![None; expected_symbols.len()];
    };
    let mut out = vec![None; expected_symbols.len()];
    for (i, entry) in expected_symbols.iter().enumerate() {
        let Some(obj) = arr.get(i) else { continue };
        let sid = obj.get("symbol_id").and_then(|v| v.as_str());
        let new_name = obj.get("new_name").and_then(|v| v.as_str());
        let expected = entry.get("symbol_id").and_then(|v| v.as_str());
        if sid == expected {
            if let Some(name) = new_name {
                if is_valid_rust_ident(name) && name.len() <= 64 {
                    out[i] = Some(name.to_string());
                }
            }
        }
    }
    out
}

fn is_valid_rust_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return false };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    !is_rust_keyword(s)
}

fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern"
            | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match"
            | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "self" | "Self"
            | "static" | "struct" | "super" | "trait" | "true" | "type" | "unsafe"
            | "use" | "where" | "while" | "async" | "await" | "dyn"
    )
}

pub(crate) fn apply_suggestions_from_stdin(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let suggestions: Vec<serde_json::Value> = serde_json::from_str(&input)?;
    let mut entries = load_symbols_entries(path)?;
    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        if let Some(sid) = entry.get("symbol_id").and_then(|v| v.as_str()) {
            index.insert(sid.to_string(), i);
        }
    }
    for suggestion in suggestions {
        let sid = suggestion.get("symbol_id").and_then(|v| v.as_str()).unwrap_or("");
        let new_name = suggestion.get("new_name").and_then(|v| v.as_str()).unwrap_or("");
        if sid.is_empty() || new_name.is_empty() {
            continue;
        }
        if !is_valid_rust_ident(new_name) {
            continue;
        }
        if let Some(&idx) = index.get(sid) {
            entries[idx]["new_name"] = serde_json::Value::String(new_name.to_string());
        }
    }
    write_symbols_entries(path, &entries)?;
    Ok(())
}

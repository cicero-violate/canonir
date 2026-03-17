use canon_agent::task_graph::TaskGraph;
use canon_agent::decompose::DecomposeNodeType;
use canon_agent::task_graph::TaskNode;
use serde_json::json;

pub(super) fn capability_name_for_node(node: &TaskNode) -> Option<&'static str> {
    node.required_capabilities.iter().find_map(|cap| cap.registry_name())
        .or_else(|| {
            if matches!(node.node_type, DecomposeNodeType::Analysis) {
                Some("llm.call")
            } else {
                None
            }
        })
}

pub(super) fn build_capability_args(node: &TaskNode, capability: &str) -> Option<serde_json::Value> {
    if capability == "llm.call" {
        let prompt = node.description.clone();
        // Analysis nodes go to the planner endpoint (returns graph patches).
        // All other nodes go to the exec endpoint (returns delta tool calls).
        let (raw, role) = if matches!(node.node_type, DecomposeNodeType::Analysis) {
            (false, "planner")
        } else {
            (true, "exec")
        };
        return Some(json!({ "prompt": prompt, "raw": raw, "role": role }));
    }
    if let Some(args) = parse_inline_json(&node.description) {
        return Some(args);
    }
    match capability {
        "file.read" => extract_path(&node.description).map(|path| json!({ "path": path })),
        "file.write" => {
            let path = extract_path(&node.description)?;
            let content = extract_field(&node.description, "content")
                .unwrap_or_else(|| String::new());
            Some(json!({ "path": path, "content": content }))
        }
        "bash" => {
            let cmd = extract_field(&node.description, "cmd")
                .unwrap_or_else(|| node.description.trim().to_string());
            Some(json!({ "cmd": cmd }))
        }
        "cargo.build" => {
            let crate_name = extract_field(&node.description, "crate")?;
            Some(json!({ "crate": crate_name }))
        }
        "cargo.check" => {
            let crate_name = extract_field(&node.description, "crate")?;
            Some(json!({ "crate": crate_name }))
        }
        _ => None,
    }
}

pub(super) fn parse_inline_json(text: &str) -> Option<serde_json::Value> {
    let s = parse_inline_json_str(text)?;
    serde_json::from_str(&s).ok()
}

/// Extract the outermost `{...}` span from text as a String without parsing.
pub(super) fn parse_inline_json_str(text: &str) -> Option<String> {
    let start = text.find('{')?;
    // Walk forward from start to find the matching closing brace
    let mut depth = 0usize;
    let mut end = None;
    let chars = text[start..].char_indices();
    for (i, c) in chars {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    Some(text[start..=end].to_string())
}

/// Parse executor-format LLM response. Returns Some(deltas) if the value contains
/// a `{"results":[{"deltas":[...]}]}` structure. Returns None if not executor format.
/// Returns Some(empty vec) if executor format but no tool calls (done).
pub(super) fn parse_executor_deltas(val: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
    let results = val.get("results")?.as_array()?;
    let deltas: Vec<serde_json::Value> = results
        .iter()
        .filter_map(|r| r.get("deltas")?.as_array().cloned())
        .flatten()
        .collect();
    Some(deltas)
}

/// Convert an executor delta into a bash `cmd` string.
pub(super) fn delta_to_cap_args(delta: &serde_json::Value) -> serde_json::Value {
    let kind = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let cmd = match kind {
        "read_file" => {
            let path = delta.get("path").and_then(|v| v.as_str()).unwrap_or("/");
            format!("cat {}", shell_quote(path))
        }
        "list_dir" => {
            let path = delta.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            format!("ls -la {}", shell_quote(path))
        }
        "read_command" => {
            let command = delta.get("command").and_then(|v| v.as_str()).unwrap_or("echo");
            let args = delta.get("args")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter()
                    .filter_map(|a| a.as_str())
                    .map(shell_quote)
                    .collect::<Vec<_>>()
                    .join(" "))
                .unwrap_or_default();
            let path = delta.get("path").and_then(|v| v.as_str());
            if let Some(p) = path {
                format!("cd {} && {} {}", shell_quote(p), command, args)
            } else {
                format!("{} {}", command, args)
            }
        }
        _ => format!("echo 'unknown delta type: {}'", kind),
    };
    serde_json::json!({ "cmd": cmd })
}

pub(super) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub(super) fn extract_path(text: &str) -> Option<String> {
    extract_field(text, "path").or_else(|| {
        text.split_whitespace()
            .find(|tok| tok.starts_with('/') || tok.starts_with("./"))
            .map(|tok| tok.to_string())
    })
}

pub(super) fn extract_field(text: &str, key: &str) -> Option<String> {
    let pattern = format!("{key}=");
    if let Some(idx) = text.find(&pattern) {
        let value = &text[idx + pattern.len()..];
        return Some(value.split_whitespace().next().unwrap_or("").trim().to_string());
    }
    let pattern = format!("{key}:");
    if let Some(idx) = text.find(&pattern) {
        let value = &text[idx + pattern.len()..];
        return Some(value.split_whitespace().next().unwrap_or("").trim().to_string());
    }
    None
}

pub(super) fn parse_node_id_from_request_id(request_id: &str) -> Option<String> {
    let rest = request_id.strip_prefix("node-")?;
    let last_dash = rest.rfind('-')?;
    if last_dash == 0 {
        return None;
    }
    Some(rest[..last_dash].to_string())
}

pub(super) fn unique_node_id(base: &str, graph: &TaskGraph) -> String {
    if graph.nodes.iter().all(|n| n.id != base) {
        return base.to_string();
    }
    let mut idx = 1u32;
    loop {
        let candidate = format!("{base}_{idx}");
        if graph.nodes.iter().all(|n| n.id != candidate) {
            return candidate;
        }
        idx = idx.saturating_add(1);
    }
}

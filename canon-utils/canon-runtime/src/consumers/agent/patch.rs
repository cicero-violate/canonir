use canon_agent::task_graph_patch::{TaskGraphEvent, TaskGraphPatch};
use canon_event::{CanonEvent, GoalNodeCreated, GoalNodeRetracted, GoalNodeRewritten, GoalEdgeDefined};

pub(super) fn extract_graph_patch_from_llm_result(
    result: &serde_json::Value,
) -> Result<TaskGraphPatch, String> {
    // Executor format ({"results": [...]}) is not a TaskGraphPatch.
    if result.get("results").is_some() {
        return Err("executor format: result contains `results`".to_string());
    }
    // Both raw=false (planner) and raw=true (executor) now deliver a parsed JSON Value.
    // Try to deserialise directly as TaskGraphPatch.
    if let Ok(patch) = serde_json::from_value::<TaskGraphPatch>(result.clone()) {
        return Ok(patch);
    }

    // Common LLM wrapper: { "text": "```json ... ```" }
    if let Some(text) = result.get("text").and_then(|v| v.as_str()) {
        return parse_patch_from_text(text);
    }

    // Or plain string payload.
    if let Some(text) = result.as_str() {
        return parse_patch_from_text(text);
    }

    Err("result did not match TaskGraphPatch schema".to_string())
}

/// Emit TaskGraphEvent mutations as RuntimeEvents via the emitter.
pub(super) fn emit_goal_graph_events(emitter: &canon_event::EventEmitterHandle, events: Vec<TaskGraphEvent>) {
    for event in events {
        let runtime_event = match event {
            TaskGraphEvent::NodeCreated { node_id, description, deps, caps, node_type, priority, budget } => {
                CanonEvent::GoalNodeCreated(GoalNodeCreated { node_id, description, deps, caps, node_type, priority, budget })
            }
            TaskGraphEvent::NodeRetracted { node_id } => {
                CanonEvent::GoalNodeRetracted(GoalNodeRetracted { node_id })
            }
            TaskGraphEvent::NodeRewritten { node_id, new_description, new_caps } => {
                CanonEvent::GoalNodeRewritten(GoalNodeRewritten { node_id, new_description, new_caps })
            }
            TaskGraphEvent::EdgeDefined { from, to } => {
                CanonEvent::GoalEdgeDefined(GoalEdgeDefined { from_node_id: from, to_node_id: to })
            }
        };
        emitter.emit(runtime_event);
    }
}

fn parse_patch_from_text(text: &str) -> Result<TaskGraphPatch, String> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let value: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("parse json from text: {e}"))?;
        return serde_json::from_value::<TaskGraphPatch>(value)
            .map_err(|e| format!("decode TaskGraphPatch: {e}"));
    }
    if let Some(json) = extract_fenced_json(trimmed) {
        let value: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| format!("parse fenced json: {e}"))?;
        return serde_json::from_value::<TaskGraphPatch>(value)
            .map_err(|e| format!("decode TaskGraphPatch: {e}"));
    }
    Err("no json object found in text".to_string())
}

fn extract_fenced_json(text: &str) -> Option<String> {
    let mut start = None;
    let mut end = None;
    let mut lines = text.lines();
    let mut idx = 0usize;
    while let Some(line) = lines.next() {
        let l = line.trim_start();
        if start.is_none() && (l.starts_with("```json") || l == "```") {
            start = Some(idx + 1);
        } else if start.is_some() && l == "```" {
            end = Some(idx);
            break;
        }
        idx += 1;
    }
    let start = start?;
    let end = end.unwrap_or(idx + 1);
    let json = text
        .lines()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect::<Vec<_>>()
        .join("\n");
    let json = json.trim();
    if json.is_empty() {
        None
    } else {
        Some(json.to_string())
    }
}

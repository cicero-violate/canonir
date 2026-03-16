use canon_agent::goal_patch::{GoalGraphEvent, GoalGraphPatch};
use canon_event::RuntimeEvent;

use super::executor::{extract_fenced_json, parse_inline_json_str};

pub(super) fn extract_graph_patch_from_llm_result(
    result: &serde_json::Value,
) -> Option<GoalGraphPatch> {
    // raw=false (analysis/planner) path: result IS the parsed JSON value directly.
    // Try deserialising it straight into a GoalGraphPatch first.
    if result.get("text").is_none() && result.get("results").is_none() {
        if let Ok(patch) = serde_json::from_value::<GoalGraphPatch>(result.clone()) {
            return Some(patch);
        }
    }

    // raw=true (executor) path: result = {"text": "..."}. Parse the text string.
    let text = result.get("text").and_then(|v| v.as_str())?;
    // Try fenced JSON block first (```json ... ```), then fall back to brace-depth scan.
    let json_str = extract_fenced_json(text)
        .or_else(|| parse_inline_json_str(text))?;
    let parsed = serde_json::from_str::<serde_json::Value>(&json_str).ok()?;
    if parsed.get("results").is_some() {
        return None; // executor format — not a GoalGraphPatch
    }
    let patch = serde_json::from_value::<GoalGraphPatch>(parsed).ok()?;
    if patch.new_nodes.is_empty() && patch.new_edges.is_empty()
        && patch.retract_nodes.is_empty() && patch.rewrite_nodes.is_empty()
    {
        canon_event::emit_debug::warn(
            "agent_consumer",
            "graph_patch_empty",
            serde_json::json!({ "text_preview": &text[..text.len().min(200)] }),
        );
    }
    Some(patch)
}

/// Emit GoalGraphEvent mutations as RuntimeEvents via the emitter.
pub(super) fn emit_goal_graph_events(emitter: &canon_event::RuntimeEmitterHandle, events: Vec<GoalGraphEvent>) {
    for event in events {
        let runtime_event = match event {
            GoalGraphEvent::NodeCreated { node_id, description, deps, caps, node_type, priority, budget } => {
                RuntimeEvent::GoalNodeCreated { node_id, description, deps, caps, node_type, priority, budget }
            }
            GoalGraphEvent::NodeRetracted { node_id } => {
                RuntimeEvent::GoalNodeRetracted { node_id }
            }
            GoalGraphEvent::NodeRewritten { node_id, new_description, new_caps } => {
                RuntimeEvent::GoalNodeRewritten { node_id, new_description, new_caps }
            }
            GoalGraphEvent::EdgeDefined { from, to } => {
                RuntimeEvent::GoalEdgeDefined { from_node_id: from, to_node_id: to }
            }
        };
        emitter.emit(runtime_event);
    }
}

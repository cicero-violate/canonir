use canon_agent::task_graph_patch::{TaskGraphEvent, TaskGraphPatch};
use canon_event::CanonEvent;

pub(super) fn extract_graph_patch_from_llm_result(
    result: &serde_json::Value,
) -> Option<TaskGraphPatch> {
    // Executor format ({"results": [...]}) is not a TaskGraphPatch.
    if result.get("results").is_some() {
        return None;
    }
    // Both raw=false (planner) and raw=true (executor) now deliver a parsed JSON Value.
    // Try to deserialise directly as TaskGraphPatch.
    let patch = serde_json::from_value::<TaskGraphPatch>(result.clone()).ok()?;
    if patch.new_nodes.is_empty() && patch.new_edges.is_empty()
        && patch.retract_nodes.is_empty() && patch.rewrite_nodes.is_empty()
    {
        canon_event::emit_debug::warn(
            "agent_consumer",
            "graph_patch_empty",
            serde_json::json!({ "result_preview": serde_json::to_string(result).unwrap_or_default().chars().take(200).collect::<String>() }),
        );
    }
    Some(patch)
}

/// Emit TaskGraphEvent mutations as RuntimeEvents via the emitter.
pub(super) fn emit_goal_graph_events(emitter: &canon_event::EventEmitterHandle, events: Vec<TaskGraphEvent>) {
    for event in events {
        let runtime_event = match event {
            TaskGraphEvent::NodeCreated { node_id, description, deps, caps, node_type, priority, budget } => {
                CanonEvent::GoalNodeCreated { node_id, description, deps, caps, node_type, priority, budget }
            }
            TaskGraphEvent::NodeRetracted { node_id } => {
                CanonEvent::GoalNodeRetracted { node_id }
            }
            TaskGraphEvent::NodeRewritten { node_id, new_description, new_caps } => {
                CanonEvent::GoalNodeRewritten { node_id, new_description, new_caps }
            }
            TaskGraphEvent::EdgeDefined { from, to } => {
                CanonEvent::GoalEdgeDefined { from_node_id: from, to_node_id: to }
            }
        };
        emitter.emit(runtime_event);
    }
}

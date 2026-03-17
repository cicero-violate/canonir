use canon_agent::task_graph::{TaskGraph, NodeStatus};
use canon_event_store::{replay_goal_graph_from_tlog, replay_capability_graph_from_tlog};

use super::resolve_runtime_tlog_path;

pub(super) fn reports_out_dir() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("CANON_REPORTS_OUT") {
        return std::path::PathBuf::from(p);
    }
    std::path::PathBuf::from("/workspace/ai_sandbox/canon/state/reports_out")
}

/// Write the full reports_out/ tree, broken down recursively by tlog item type.
/// Each leaf is a single xyz.json. Intermediate directories group related items.
///
/// reports_out/
///   goal/
///     summary.json                     <- tick + node counts
///     nodes/<node_id>.json             <- one file per goal node
///     edges/<from>__<to>.json          <- one file per directed edge
///   capabilities/
///     <request_id>/
///       invoked.json                   <- capability invocation metadata
///       resolved.json                  <- resolution (success/fail/duration)
///       tools/
///         <delta_id>/
///           call.json                  <- tool_call payload
///           result.json                <- tool_result output
///   llm/
///     responses/<request_id>.json      <- one file per LLM response
pub(super) fn write_graph_report(graph: &TaskGraph, tick: u64) {
    let root = reports_out_dir();
    let tlog_path = resolve_runtime_tlog_path();

    // -- goal/ -----------------------------------------------------------------
    let goal_dir = root.join("goal");
    let nodes_dir = goal_dir.join("nodes");
    let edges_dir = goal_dir.join("edges");
    let _ = std::fs::create_dir_all(&nodes_dir);
    let _ = std::fs::create_dir_all(&edges_dir);

    // goal/nodes/<id>.json from in-memory TaskGraph (authoritative for live state)
    for n in &graph.nodes {
        let result_preview = n.result.as_deref()
            .map(|r| if r.len() > 400 { format!("{}...", &r[..400]) } else { r.to_string() });
        let v = serde_json::json!({
            "id": n.id,
            "status": format!("{:?}", n.status).to_lowercase(),
            "type": format!("{:?}", n.node_type).to_lowercase(),
            "caps": n.required_capabilities.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>(),
            "deps": n.deps,
            "priority": n.priority,
            "budget": n.budget,
            "description": n.description,
            "result_preview": result_preview,
            "error": n.error,
        });
        if let Ok(s) = serde_json::to_string_pretty(&v) {
            let _ = std::fs::write(nodes_dir.join(format!("{}.json", safe_filename(&n.id))), s);
        }
    }

    // Merge tlog-derived goal graph: adds any nodes/edges the projector knows about
    if tlog_path.exists() {
        if let Ok(gs) = replay_goal_graph_from_tlog(&tlog_path) {
            for (id, pn) in &gs.nodes {
                let path = nodes_dir.join(format!("{}.json", safe_filename(id)));
                if !path.exists() {
                    if let Ok(s) = serde_json::to_string_pretty(pn) {
                        let _ = std::fs::write(path, s);
                    }
                }
            }
            for (from, to) in &gs.edges {
                let name = format!("{}_{}.json", safe_filename(from), safe_filename(to));
                let v = serde_json::json!({ "from": from, "to": to });
                if let Ok(s) = serde_json::to_string_pretty(&v) {
                    let _ = std::fs::write(edges_dir.join(name), s);
                }
            }
        }
    }

    // goal/summary.json
    let summary = serde_json::json!({
        "tick": tick,
        "total": graph.nodes.len(),
        "completed": graph.nodes.iter().filter(|n| n.status == NodeStatus::Completed).count(),
        "running":   graph.nodes.iter().filter(|n| n.status == NodeStatus::Running).count(),
        "pending":   graph.nodes.iter().filter(|n| n.status == NodeStatus::Pending || n.status == NodeStatus::Ready).count(),
        "failed":    graph.nodes.iter().filter(|n| n.status == NodeStatus::Failed).count(),
    });
    if let Ok(s) = serde_json::to_string_pretty(&summary) {
        let _ = std::fs::write(goal_dir.join("summary.json"), s);
    }

    // -- capabilities/ ---------------------------------------------------------
    if tlog_path.exists() {
        if let Ok(cs) = replay_capability_graph_from_tlog(&tlog_path) {
            let caps_dir = root.join("capabilities");
            for (cap_id, node) in &cs.nodes {
                let cap_dir = caps_dir.join(safe_filename(cap_id));
                let _ = std::fs::create_dir_all(&cap_dir);
                let invoked = serde_json::json!({
                    "capability_id": node.capability_id,
                    "name": node.name,
                    "node_id": node.node_id,
                    "status": node.status,
                    "duration_ms": node.duration_ms,
                });
                if let Ok(s) = serde_json::to_string_pretty(&invoked) {
                    let _ = std::fs::write(cap_dir.join("invoked.json"), s);
                }
                if node.status != "pending" {
                    let resolved = serde_json::json!({
                        "capability_id": node.capability_id,
                        "success": node.status == "completed",
                        "duration_ms": node.duration_ms,
                    });
                    if let Ok(s) = serde_json::to_string_pretty(&resolved) {
                        let _ = std::fs::write(cap_dir.join("resolved.json"), s);
                    }
                }
            }
            for edge in &cs.edges {
                if edge.kind == "tool_call" {
                    let tool_dir = caps_dir
                        .join(safe_filename(&edge.from))
                        .join("tools")
                        .join(safe_filename(&edge.to));
                    let _ = std::fs::create_dir_all(&tool_dir);
                    let v = serde_json::json!({ "tool_call_id": edge.to, "parent_capability_id": edge.from });
                    if let Ok(s) = serde_json::to_string_pretty(&v) {
                        let _ = std::fs::write(tool_dir.join("call.json"), s);
                    }
                }
            }
        }
    }
}

/// Write one LLM response as a leaf file: llm/responses/<request_id>.json
pub(super) fn append_llm_response_log(node_id: &str, request_id: &str, text: &str) {
    let dir = reports_out_dir().join("llm").join("responses");
    let _ = std::fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let entry = serde_json::json!({
        "ts_ms": ts,
        "node_id": node_id,
        "request_id": request_id,
        "text": text,
    });
    if let Ok(s) = serde_json::to_string_pretty(&entry) {
        let _ = std::fs::write(dir.join(format!("{}.json", safe_filename(request_id))), s);
    }
}

/// Sanitise a string for safe use as a filesystem path component.
pub(super) fn safe_filename(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect()
}

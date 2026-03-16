use crate::reader::{read_any_events_from_path_with_start_seq, AnyEvent};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CapabilityGraphState {
    pub nodes: HashMap<String, CapabilityOpNode>,
    pub edges: Vec<CapabilityOpEdge>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CapabilityOpNode {
    pub capability_id: String,
    pub name: String,
    pub node_id: String,
    pub status: String, // "pending" | "completed" | "failed"
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CapabilityOpEdge {
    pub from: String,
    pub to: String,
    pub kind: String, // "triggers" | "tool_call"
}

pub fn replay_capability_graph_from_tlog(tlog_path: &Path) -> anyhow::Result<CapabilityGraphState> {
    let events = read_any_events_from_path_with_start_seq(tlog_path, 0)?;
    let mut state = CapabilityGraphState::default();
    // track dispatch timestamps for duration
    let mut start_times: HashMap<String, u64> = HashMap::new();
    for event in &events {
        let AnyEvent::Canon(canon) = event else { continue };
        let payload = &canon.payload;
        match canon.kind.as_str() {
            "capability_requested" => {
                let id = payload.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let node_id = payload.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if !id.is_empty() {
                    start_times.insert(id.clone(), canon.ts);
                    state.nodes.insert(id.clone(), CapabilityOpNode { capability_id: id, name, node_id, status: "pending".to_string(), duration_ms: None, result: None });
                }
            }
            "capability_completed" => {
                let id = payload.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if let Some(node) = state.nodes.get_mut(&id) {
                    node.status = "completed".to_string();
                    if let Some(start) = start_times.get(&id) {
                        node.duration_ms = Some(canon.ts.saturating_sub(*start));
                    }
                }
            }
            "capability_failed" => {
                let id = payload.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if let Some(node) = state.nodes.get_mut(&id) { node.status = "failed".to_string(); }
            }
            "tool_call" => {
                let req_id = payload.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let node_id = payload.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let kind = payload.get("kind").and_then(|v| v.as_str()).unwrap_or("tool").to_string();
                if !req_id.is_empty() {
                    // tool_call is a child edge from the parent capability (node_id's capability)
                    state.edges.push(CapabilityOpEdge { from: node_id, to: req_id.clone(), kind: "tool_call".to_string() });
                    state.nodes.entry(req_id.clone()).or_insert_with(|| CapabilityOpNode { capability_id: req_id, name: kind, node_id: String::new(), status: "pending".to_string(), duration_ms: None, result: None });
                }
            }
            "tool_result" => {
                let req_id = payload.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let success = payload.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                let output = payload.get("output").cloned();
                if !req_id.is_empty() {
                    if let Some(node) = state.nodes.get_mut(&req_id) {
                        node.status = if success { "completed" } else { "failed" }.to_string();
                        node.result = output;
                        if let Some(start) = start_times.get(&req_id) {
                            node.duration_ms = Some(canon.ts.saturating_sub(*start));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(state)
}

use crate::reader::{read_any_events_from_path_with_start_seq, AnyEvent};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GoalGraphState {
    pub nodes: HashMap<String, GoalNodeState>,
    pub edges: Vec<(String, String)>, // (from, to)
    pub seq_processed: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalNodeState {
    pub node_id: String,
    pub description: String,
    pub deps: Vec<String>,
    pub caps: Vec<String>,
    pub node_type: String,
    pub priority: u8,
    pub budget: Option<u32>,
    pub status: String, // "pending" | "running" | "completed" | "failed"
}

pub fn replay_goal_graph_from_tlog(tlog_path: &Path) -> anyhow::Result<GoalGraphState> {
    replay_goal_graph_incremental(tlog_path, 0, GoalGraphState::default())
}

pub fn replay_goal_graph_incremental(tlog_path: &Path, start_seq: u64, mut state: GoalGraphState) -> anyhow::Result<GoalGraphState> {
    let events = read_any_events_from_path_with_start_seq(tlog_path, start_seq)?;
    for event in &events {
        apply_planning_event(event, &mut state);
    }
    Ok(state)
}

fn apply_planning_event(event: &AnyEvent, state: &mut GoalGraphState) {
    let AnyEvent::Canon(canon) = event else { return };
    let payload = &canon.payload;
    match canon.kind.as_str() {
        "goal_node_created" => {
            let node_id = payload.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if node_id.is_empty() {
                return;
            }
            state.nodes.insert(
                node_id.clone(),
                GoalNodeState {
                    node_id,
                    description: payload.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    deps: payload.get("deps").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                    caps: payload.get("caps").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                    node_type: payload.get("node_type").and_then(|v| v.as_str()).unwrap_or("analysis").to_string(),
                    priority: payload.get("priority").and_then(|v| v.as_u64()).unwrap_or(1) as u8,
                    budget: payload.get("budget").and_then(|v| v.as_u64()).map(|v| v as u32),
                    status: "pending".to_string(),
                },
            );
        }
        "goal_node_retracted" => {
            let node_id = payload.get("node_id").and_then(|v| v.as_str()).unwrap_or("");
            state.nodes.remove(node_id);
            state.edges.retain(|(f, t)| f != node_id && t != node_id);
        }
        "goal_node_rewritten" => {
            let node_id = payload.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if let Some(node) = state.nodes.get_mut(&node_id) {
                if let Some(d) = payload.get("new_description").and_then(|v| v.as_str()) {
                    node.description = d.to_string();
                }
                if let Some(caps) = payload.get("new_caps").and_then(|v| v.as_array()) {
                    node.caps = caps.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
                }
                node.status = "pending".to_string();
            }
        }
        "goal_edge_defined" => {
            let from = payload.get("from_node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let to = payload.get("to_node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !from.is_empty() && !to.is_empty() && !state.edges.contains(&(from.clone(), to.clone())) {
                state.edges.push((from, to));
            }
        }
        "node_started" => {
            let node_id = payload.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if let Some(node) = state.nodes.get_mut(&node_id) {
                node.status = "running".to_string();
            }
        }
        "node_completed" => {
            let node_id = payload.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if let Some(node) = state.nodes.get_mut(&node_id) {
                node.status = "completed".to_string();
            }
        }
        "node_failed" => {
            let node_id = payload.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if let Some(node) = state.nodes.get_mut(&node_id) {
                node.status = "failed".to_string();
            }
        }
        _ => {}
    }
    // Use ts as a proxy for seq_processed
    if canon.ts > state.seq_processed {
        state.seq_processed = canon.ts;
    }
}

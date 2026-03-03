//! Verifier (V) — independently validates execution and updates DAG state.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

use super::dag::{Status, TaskGraph};
use super::llm::{call_agent_json_with_retry, schema_mismatch_retry_prompt};
use crate::ws_server::WsBridge;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    pub updated_graph: TaskGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VerifyUpdate {
    pub id: String,
    pub status: Status,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VerifyOutput {
    pub updates: Vec<VerifyUpdate>,
}

pub async fn verify_graph(
    graph: &TaskGraph,
    bridge: &WsBridge,
    url: &str,
    system_prompt: &str,
    tabs: &tokio::sync::Mutex<super::llm::DagTabSlots>,
    log_dir: &Path,
    iter: u64,
) -> Result<VerifyResult> {
    let running: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.status == Status::Running)
        .map(|n| serde_json::json!({"id": n.id, "status": n.status, "result": n.result, "description": n.description}))
        .collect();
    if running.is_empty() {
        return Ok(VerifyResult { updated_graph: graph.clone() });
    }

    let input = serde_json::json!({ "nodes": running });
    let prompt = format!("Verify running nodes and return status updates.\nRespond with JSON only.\n\nINPUT:\n{}", serde_json::to_string_pretty(&input).unwrap_or_default());

    let payload: Value = call_agent_json_with_retry(bridge, url, "verifier", &prompt, system_prompt, tabs, 3).await?;
    let output: VerifyOutput = match serde_json::from_value(payload.clone()) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = schema_mismatch_retry_prompt("verifier", &serde_json::to_string_pretty(&input).unwrap_or_default(), &payload);
            let retry_payload: Value = call_agent_json_with_retry(bridge, url, "verifier", &retry_prompt, system_prompt, tabs, 1).await?;
            serde_json::from_value(retry_payload.clone()).context("verifier output did not match schema")?
        }
    };

    let verify_path = log_dir.join(format!("iter_{:03}_verify_output.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(verify_path, pretty);
    }

    let mut updated = graph.clone();
    for upd in output.updates {
        if let Err(e) = updated.update_status(&upd.id, upd.status) {
            if let Some(node) = updated.get_node_mut(&upd.id) {
                node.error = Some(e);
            }
            continue;
        }
        if let Some(node) = updated.get_node_mut(&upd.id) {
            node.error = upd.error;
        }
    }

    let after_path = log_dir.join(format!("iter_{:03}_task_graph_after.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&updated) {
        let _ = std::fs::write(after_path, pretty);
    }

    Ok(VerifyResult { updated_graph: updated })
}

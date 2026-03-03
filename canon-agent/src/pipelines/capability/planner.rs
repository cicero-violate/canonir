use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;

use super::dag::TaskNode;
use super::llm::call_agent_json_with_retry;
use crate::ws_server::WsBridge;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgePlan {
    #[serde(default)]
    pub edges: Vec<EdgeSpec>,
}

pub async fn plan_edges(
    nodes: &[TaskNode],
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::llm::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    workspace_root: &Path,
    workspace_listing: &str,
    constraint_note: Option<&str>,
    log_dir: &Path,
    retries: u32,
    delay_secs: u64,
) -> Result<EdgePlan> {
    let summaries: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            let mut summary = n.description.clone();
            if summary.len() > 160 {
                summary.truncate(157);
                summary.push_str("...");
            }
            serde_json::json!({
                "id": n.id,
                "summary": summary,
                "required_capabilities": n.required_capabilities,
                "node_type": n.node_type
            })
        })
        .collect();
    let input = serde_json::json!({
        "nodes": summaries
    });
    let schema = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"edges\": [\n    { \"from\": \"node_id\", \"to\": \"node_id\" }\n  ]\n}\n";
    let constraints = constraint_note.unwrap_or("none");
    let prompt = format!(
        "{}\nLink the provided node ids into a DAG using edges only. Do not create, rename, or annotate nodes.\nIt is acceptable to leave some nodes unlinked if they are semantically irrelevant.\nOutput only edges; no extra fields or prose.\nWorkspace root: {}\nWorkspace entries: {}\nConstraints: {}\nAction space: you may only reference the provided node ids.\nINPUT:\n{}",
        schema,
        workspace_root.display(),
        workspace_listing,
        constraints,
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );
    let mut payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &prompt, role_schema, "planner", tabs, reuse_tabs, max_tabs, retries, delay_secs).await?;
    let mut plan: EdgePlan = match serde_json::from_value(payload.clone()) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = format!(
                "Your response did not match the schema.\n{}\n\nInvalid response:\n{}\n\nOriginal input:\n{}",
                schema,
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
                serde_json::to_string_pretty(&input).unwrap_or_default()
            );
            let retry_payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &retry_prompt, role_schema, "planner", tabs, reuse_tabs, max_tabs, 1, delay_secs).await?;
            payload = retry_payload.clone();
            serde_json::from_value(retry_payload.clone()).context("planner output did not match edge schema")?
        }
    };

    let path = log_dir.join("planner_output.json");
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, pretty);
    }

    Ok(plan)
}

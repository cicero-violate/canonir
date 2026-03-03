//! DAG planner (P) — converts task specs into TaskGraph.

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;

use super::dag::TaskGraph;
use super::decompose::TaskSpec;
use super::llm::{call_agent_json_with_retry, schema_mismatch_retry_prompt};
use crate::ws_server::WsBridge;

pub async fn plan_dag(
    tasks: &[TaskSpec],
    bridge: &WsBridge,
    url: &str,
    system_prompt: &str,
    tabs: &tokio::sync::Mutex<super::llm::DagTabSlots>,
    log_dir: &Path,
) -> Result<TaskGraph> {
    let input = serde_json::json!({ "tasks": tasks });
    let prompt = format!("Construct a TaskGraph from tasks.\nRespond with JSON only.\n\nINPUT:\n{}", serde_json::to_string_pretty(&input).unwrap_or_default());

    let payload: Value = call_agent_json_with_retry(bridge, url, "planner", &prompt, system_prompt, tabs, 3).await?;
    let graph: TaskGraph = match serde_json::from_value(payload.clone()) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = schema_mismatch_retry_prompt("planner", &serde_json::to_string_pretty(&input).unwrap_or_default(), &payload);
            let retry_payload: Value = call_agent_json_with_retry(bridge, url, "planner", &retry_prompt, system_prompt, tabs, 1).await?;
            serde_json::from_value(retry_payload.clone()).context("planner output did not match TaskGraph schema")?
        }
    };
    graph.validate().map_err(|e| anyhow::anyhow!(e))?;

    let path = log_dir.join("planner_output.json");
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, pretty);
    }

    Ok(graph)
}

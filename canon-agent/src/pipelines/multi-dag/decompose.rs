//! Goal Decomposer (D_g) — transforms GoalSpec into task specs.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

use super::goal::GoalSpec;
use super::llm::{call_agent_json_with_retry, schema_mismatch_retry_prompt};
use crate::ws_server::WsBridge;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecomposeOutput {
    pub tasks: Vec<TaskSpec>,
}

pub async fn decompose_goal(
    goal: &GoalSpec,
    bridge: &WsBridge,
    url: &str,
    system_prompt: &str,
    tabs: &tokio::sync::Mutex<super::llm::DagTabSlots>,
    log_dir: &Path,
) -> Result<DecomposeOutput> {
    let input = serde_json::json!({ "goal": goal.to_prompt_string() });
    let prompt = format!(
        "Decompose goal into tasks.\n\
Respond with exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{ \"id\": \"t1\", \"description\": \"string\", \"deps\": [] }}\n  ]\n}}\n\n\
INPUT:\n{}",
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );

    let payload: Value = call_agent_json_with_retry(bridge, url, "decompose", &prompt, system_prompt, tabs, 3).await?;
    let output: DecomposeOutput = match serde_json::from_value(payload.clone()) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = schema_mismatch_retry_prompt("decompose", &serde_json::to_string_pretty(&input).unwrap_or_default(), &payload);
            let retry_payload: Value = call_agent_json_with_retry(bridge, url, "decompose", &retry_prompt, system_prompt, tabs, 1).await?;
            serde_json::from_value(retry_payload.clone()).context("D_g output did not match schema")?
        }
    };

    let path = log_dir.join("decompose_output.json");
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, pretty);
    }

    Ok(output)
}

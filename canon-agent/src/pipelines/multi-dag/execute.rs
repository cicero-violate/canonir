//! Executor (X) — proposes and applies deltas for ready DAG nodes.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

use super::act::apply_mutations;
use super::dag::{Status, TaskGraph};
use super::llm::{call_agent_json_with_retry, schema_mismatch_retry_prompt};
use super::Delta;
use crate::ws_server::WsBridge;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResult {
    pub updated_graph: TaskGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExecNodeResult {
    pub id: String,
    #[serde(default)]
    pub deltas: Vec<Delta>,
    #[serde(default)]
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExecOutput {
    #[serde(default)]
    pub results: Vec<ExecNodeResult>,
}

pub async fn execute_ready(
    graph: &TaskGraph,
    bridge: &WsBridge,
    url: &str,
    system_prompt: &str,
    tabs: &tokio::sync::Mutex<super::llm::DagTabSlots>,
    log_dir: &Path,
    iter: u64,
    roots: &[std::path::PathBuf],
    max_output_lines: usize,
) -> Result<ExecuteResult> {
    let ready: Vec<_> = graph.ready_nodes().iter().map(|n| serde_json::json!({"id": n.id, "description": n.description})).collect();
    if ready.is_empty() {
        return Ok(ExecuteResult { updated_graph: graph.clone() });
    }

    let input = serde_json::json!({ "nodes": ready });
    let prompt = format!(
        "Propose deltas for ready nodes.\n\
Respond with exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"results\": [\n    {{ \"id\": \"t1\", \"deltas\": [ {{ \"type\": \"write_file\", \"path\": \"x\", \"content\": \"...\" }} ], \"rationale\": \"string\" }}\n  ]\n}}\n\n\
INPUT:\n{}",
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );

    let payload: Value = call_agent_json_with_retry(bridge, url, "executor", &prompt, system_prompt, tabs, 3).await?;
    let exec_path = log_dir.join(format!("iter_{:03}_execute_output.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(exec_path, pretty);
    }
    let output: ExecOutput = match serde_json::from_value(payload.clone()) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = schema_mismatch_retry_prompt("executor", &serde_json::to_string_pretty(&input).unwrap_or_default(), &payload);
            let retry_payload: Value = call_agent_json_with_retry(bridge, url, "executor", &retry_prompt, system_prompt, tabs, 1).await?;
            serde_json::from_value(retry_payload.clone()).context("executor output did not match schema")?
        }
    };

    let before_path = log_dir.join(format!("iter_{:03}_task_graph_before.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&graph) {
        let _ = std::fs::write(before_path, pretty);
    }
    let mut updated = graph.clone();
    for result in output.results {
        let (out, _results, err) = apply_mutations(&result.deltas, roots, max_output_lines);
        let _ = updated.update_status(&result.id, Status::Running);
        if let Some(node) = updated.get_node_mut(&result.id) {
            node.result = Some(out);
            node.error = err;
        }
    }

    Ok(ExecuteResult { updated_graph: updated })
}

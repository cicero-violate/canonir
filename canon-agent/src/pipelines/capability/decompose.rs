use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

use super::capability::Capability;
use super::config::GoalSpec;
use super::llm::call_agent_json_with_retry_allow_mismatch;
use super::tab_management::TabsHandle;
use crate::ws_server::WsBridge;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    #[default]
    Analysis,
    Render,
}

fn default_node_type() -> NodeType {
    NodeType::Analysis
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
    #[serde(default = "default_node_type")]
    pub node_type: NodeType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecomposeOutput {
    pub tasks: Vec<TaskSpec>,
}

async fn decompose_inner(
    prompt: String,
    input: Value,
    schema: &str,
    caps: &[&str],
    phase: &str,
    log_path: std::path::PathBuf,
    min_tasks: Option<usize>,
    min_tasks_message: Option<usize>,
    retry_on_parse: bool,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    role_schema: &str,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<DecomposeOutput> {
    let mut payload: Value = call_agent_json_with_retry_allow_mismatch(
        bridge,
        endpoint_id,
        url,
        stateful,
        &prompt,
        role_schema,
        phase,
        None,
        tabs,
        max_tabs,
        tab_cooldown_ms,
        retries,
        delay_secs,
    )
    .await?;

    let mut output: DecomposeOutput = if retry_on_parse {
        match serde_json::from_value(payload.clone()) {
            Ok(v) => v,
            Err(_) => {
                let retry_prompt = format!(
                    "Your response did not match the schema.\n\
Return exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{\n      \"id\": \"t1\",\n      \"description\": \"string\",\n      \"deps\": [],\n      \"required_capabilities\": [\"{}\"],\n      \"node_type\": \"analysis|render\"\n    }}\n  ]\n}}\n\
Allowed capability values:\n{}\n\n\
Invalid response:\n{}\n\n\
Original input:\n{}",
                    "file_write",
                    caps.join(", "),
                    serde_json::to_string_pretty(&payload).unwrap_or_default(),
                    serde_json::to_string_pretty(&input).unwrap_or_default()
                );
                let retry_payload: Value = call_agent_json_with_retry_allow_mismatch(
                    bridge,
                    endpoint_id,
                    url,
                    stateful,
                    &retry_prompt,
                    role_schema,
                    phase,
                    None,
                    tabs,
                    max_tabs,
                    tab_cooldown_ms,
                    1,
                    delay_secs,
                )
                .await?;
                payload = retry_payload.clone();
                serde_json::from_value(retry_payload.clone()).context("D_g output did not match schema")?
            }
        }
    } else {
        serde_json::from_value(payload.clone()).context("D_g output did not match schema")?
    };

    if let Some(min_tasks) = min_tasks {
        if output.tasks.len() < min_tasks {
            let min_tasks_message = min_tasks_message.unwrap_or(min_tasks);
            let retry_prompt = format!(
                "Your response must include at least {} tasks.\n\
Return exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{\n      \"id\": \"t1\",\n      \"description\": \"string\",\n      \"deps\": [],\n      \"required_capabilities\": [\"{}\"],\n      \"node_type\": \"analysis|render\"\n    }}\n  ]\n}}\n\
Allowed capability values:\n{}\n\n\
Invalid response:\n{}\n\n\
Original input:\n{}",
                min_tasks_message,
                "file_write",
                caps.join(", "),
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
                serde_json::to_string_pretty(&input).unwrap_or_default()
            );
            let retry_payload: Value = call_agent_json_with_retry_allow_mismatch(
                bridge,
                endpoint_id,
                url,
                stateful,
                &retry_prompt,
                role_schema,
                phase,
                None,
                tabs,
                max_tabs,
                tab_cooldown_ms,
                1,
                delay_secs,
            )
            .await?;
            payload = retry_payload.clone();
            let retry_output: DecomposeOutput =
                serde_json::from_value(retry_payload.clone()).context("D_g output did not match schema")?;
            if retry_output.tasks.len() < min_tasks {
                return Err(anyhow::anyhow!("D_g returned too few tasks"));
            }
            output = retry_output;
        }
    }

    for t in &mut output.tasks {
        t.node_type = normalize_node_type(t.node_type, &t.required_capabilities, &t.description);
    }

    let has_render = output.tasks.iter().any(|t| t.node_type == NodeType::Render);
    if !has_render {
        let render_hint = format!(
            "Your decomposition has no render nodes. At least one task must have node_type=render \
and include file_write or apply_patch in required_capabilities.\n\
Return exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{\n      \"id\": \"t1\",\n      \"description\": \"string\",\n      \"deps\": [],\n      \
\"required_capabilities\": [\"file_write\"],\n      \"node_type\": \"render\"\n    }}\n  ]\n}}\n\
Allowed capability values:\n{}\n\nOriginal input:\n{}",
            caps.join(", "),
            serde_json::to_string_pretty(&input).unwrap_or_default()
        );
        let retry_payload: Value = call_agent_json_with_retry_allow_mismatch(
            bridge, endpoint_id, url, stateful, &render_hint, role_schema, phase,
            None, tabs, max_tabs, tab_cooldown_ms, 1, delay_secs,
        ).await?;
        let retry_output: DecomposeOutput =
            serde_json::from_value(retry_payload.clone()).context("render retry did not match schema")?;
        let mut merged = output.tasks.clone();
        for mut t in retry_output.tasks {
            t.node_type = normalize_node_type(t.node_type, &t.required_capabilities, &t.description);
            if !merged.iter().any(|e| e.id == t.id) {
                merged.push(t);
            }
        }
        output.tasks = merged;
    }

    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(log_path, pretty);
    }

    Ok(output)
}

pub async fn decompose_goal(
    goal: &GoalSpec,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    role_schema: &str,
    tabs: &TabsHandle,
    max_tabs: usize,
    workspace_root: &Path,
    workspace_listing: &str,
    log_dir: &Path,
    retries: u32,
    delay_secs: u64,
    tab_cooldown_ms: u64,
) -> Result<DecomposeOutput> {
    let input = serde_json::json!({ "goal": goal.raw });
    let caps = [
        "create_node","add_edge","update_status","read_dag","schedule_ready",
        "goal_to_subgoals","constraint_attach","refine_node","dependency_rewrite","radius_budget_eval",
        "apply_patch","file_read","file_write","bash","cargo_build","cargo_check","stdout_capture",
        "parse_orchestration_report","detect_failures","status_update_only",
        "read_structural_surface","compute_delta","reward_signal_compute",
        "invariant_check","boundary_guard","prompt_contract_enforce","stateless_invoke"
    ];
    let schema = format!(
        "Return exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{\n      \"id\": \"t1\",\n      \"description\": \"string\",\n      \"deps\": [],\n      \"required_capabilities\": [\"{}\"],\n      \"node_type\": \"analysis|render\"\n    }}\n  ]\n}}\n\
Allowed capability values (use only these):\n{}",
        "file_write",
        caps.join(", ")
    );
    let prompt = format!(
        "Decompose goal into tasks (nodes only, no dependencies).\n\
Return at least 3 tasks. Use unique task ids.\n\
Continue decomposition until leaf tasks are pure functions suitable as GPU kernel functions (no side effects; control flow lives in the DAG).\n\
Leaf tasks that write files must have node_type=render AND include file_write or apply_patch in required_capabilities.\n\
All other tasks must have node_type=analysis and must NOT include file_write or apply_patch.\n\
Every goal that produces output files MUST have at least one render node with file_write capability.\n\
Workspace root: {}\n\
Workspace entries: {}\n\
Action space: you may only reference paths under the workspace root.\n{}\n\nINPUT:\n{}",
        workspace_root.display(),
        workspace_listing,
        schema,
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );
    decompose_inner(
        prompt,
        input,
        &schema,
        &caps,
        "decompose_goal",
        log_dir.join("decompose_output.json"),
        Some(2),
        Some(3),
        true,
        bridge,
        endpoint_id,
        url,
        stateful,
        role_schema,
        tabs,
        max_tabs,
        tab_cooldown_ms,
        retries,
        delay_secs,
    )
    .await
}

pub async fn decompose_node(
    node: TaskSpec,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    role_schema: &str,
    tabs: &TabsHandle,
    max_tabs: usize,
    workspace_root: &Path,
    workspace_listing: &str,
    log_dir: &Path,
    retries: u32,
    delay_secs: u64,
    tab_cooldown_ms: u64,
) -> Result<DecomposeOutput> {
    let input = serde_json::json!({ "node": { "id": node.id, "description": node.description } });
    let caps = [
        "create_node","add_edge","update_status","read_dag","schedule_ready",
        "goal_to_subgoals","constraint_attach","refine_node","dependency_rewrite","radius_budget_eval",
        "apply_patch","file_read","file_write","bash","cargo_build","cargo_check","stdout_capture",
        "parse_orchestration_report","detect_failures","status_update_only",
        "read_structural_surface","compute_delta","reward_signal_compute",
        "invariant_check","boundary_guard","prompt_contract_enforce","stateless_invoke"
    ];
    let schema = format!(
        "Return exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{\n      \"id\": \"t1\",\n      \"description\": \"string\",\n      \"deps\": [],\n      \"required_capabilities\": [\"{}\"]\n    }}\n  ]\n}}\n\
Allowed capability values (use only these):\n{}",
        "file_write",
        caps.join(", ")
    );
    let prompt = format!(
        "Refine the node into smaller tasks (nodes only, no dependencies).\n\
Return 2-6 child tasks. Use unique ids.\n\
Continue decomposition until leaf tasks are pure functions suitable as GPU kernel functions (no side effects; control flow lives in the DAG).\n\
Leaf tasks that write files must have node_type=render AND include file_write or apply_patch in required_capabilities.\n\
All other tasks must have node_type=analysis and must NOT include file_write or apply_patch.\n\
Workspace root: {}\nWorkspace entries: {}\nAction space: paths must be under workspace root.\n{}\n\nINPUT:\n{}",
        workspace_root.display(),
        workspace_listing,
        schema,
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );
    decompose_inner(
        prompt,
        input,
        &schema,
        &caps,
        "decompose_node",
        log_dir.join(format!("decompose_{}.json", node.id)),
        None,
        None,
        false,
        bridge,
        endpoint_id,
        url,
        stateful,
        role_schema,
        tabs,
        max_tabs,
        tab_cooldown_ms,
        retries,
        delay_secs,
    )
    .await
}

fn normalize_node_type(node_type: NodeType, caps: &[Capability], description: &str) -> NodeType {
    let render_cap = caps.iter().any(|c| matches!(c, Capability::FileWrite | Capability::ApplyPatch));
    if render_cap {
        NodeType::Render
    } else {
        NodeType::Analysis
    }
}

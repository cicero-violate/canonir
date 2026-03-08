use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use super::capability::PipelineCapability;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DecomposeNodeType {
    #[default]
    Analysis,
    Render,
}
fn decompose_default_node_type() -> DecomposeNodeType {
    DecomposeNodeType::Analysis
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecomposeTaskSpec {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<PipelineCapability>,
    #[serde(default = "decompose_default_node_type")]
    pub node_type: DecomposeNodeType,
    #[serde(default)]
    pub priority: u8,
    #[serde(default)]
    pub budget: Option<u32>,
    #[serde(default)]
    pub reasoning_trace: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecomposeDecomposeOutput {
    pub tasks: Vec<DecomposeTaskSpec>,
}
pub struct DecomposeDecomposeRequest {
    pub prompt: String,
    pub input: Value,
    pub schema: String,
    pub caps: Vec<String>,
    pub phase: &'static str,
    pub log_path: std::path::PathBuf,
    pub min_tasks: Option<usize>,
    pub min_tasks_message: Option<usize>,
    pub retry_on_parse: bool,
}
pub enum DecomposeDecomposeRetry {
    Retry { prompt: String },
    EnsureRender { prompt: String, original: DecomposeDecomposeOutput },
}
pub fn build_goal_decompose_request(
    goal_raw: &str,
    workspace_root: &Path,
    workspace_listing: &str,
    log_dir: &Path,
) -> DecomposeDecomposeRequest {
    let input = serde_json::json!({ "goal" : goal_raw });
    let caps = vec![
        "create_node", "add_edge", "update_status", "read_dag", "schedule_ready",
        "goal_to_subgoals", "constraint_attach", "refine_node", "dependency_rewrite",
        "radius_budget_eval", "apply_patch", "file_read", "file_write", "bash",
        "cargo_build", "cargo_check", "stdout_capture", "parse_orchestration_report",
        "detect_failures", "status_update_only", "read_structural_surface",
        "compute_delta", "reward_signal_compute", "invariant_check", "boundary_guard",
        "prompt_contract_enforce", "stateless_invoke",
    ];
    let schema = format!(
        "Return exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{\n      \"id\": \"t1\",\n      \"description\": \"string\",\n      \"deps\": [],\n      \"required_capabilities\": [\"{}\"],\n      \"node_type\": \"analysis|render\",\n      \"priority\": 0,\n      \"budget\": 3,\n      \"reasoning_trace\": \"string\"\n    }}\n  ]\n}}\n\
Allowed capability values (use only these):\n{}",
        "file_write", caps.join(", ")
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
        workspace_root.display(), workspace_listing, schema,
        serde_json::to_string_pretty(& input).unwrap_or_default()
    );
    DecomposeDecomposeRequest {
        prompt,
        input,
        schema,
        caps: caps.into_iter().map(|s| s.to_string()).collect(),
        phase: "decompose_goal",
        log_path: log_dir.join("decompose_output.json"),
        min_tasks: Some(2),
        min_tasks_message: Some(3),
        retry_on_parse: true,
    }
}
pub fn build_node_decompose_request(
    node: DecomposeTaskSpec,
    workspace_root: &Path,
    workspace_listing: &str,
    log_dir: &Path,
) -> DecomposeDecomposeRequest {
    let input = serde_json::json!(
        { "node" : { "id" : node.id, "description" : node.description } }
    );
    let caps = vec![
        "create_node", "add_edge", "update_status", "read_dag", "schedule_ready",
        "goal_to_subgoals", "constraint_attach", "refine_node", "dependency_rewrite",
        "radius_budget_eval", "apply_patch", "file_read", "file_write", "bash",
        "cargo_build", "cargo_check", "stdout_capture", "parse_orchestration_report",
        "detect_failures", "status_update_only", "read_structural_surface",
        "compute_delta", "reward_signal_compute", "invariant_check", "boundary_guard",
        "prompt_contract_enforce", "stateless_invoke",
    ];
    let schema = format!(
        "Return exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{\n      \"id\": \"t1\",\n      \"description\": \"string\",\n      \"deps\": [],\n      \"required_capabilities\": [\"{}\"]\n    }}\n  ]\n}}\n\
Allowed capability values (use only these):\n{}",
        "file_write", caps.join(", ")
    );
    let prompt = format!(
        "Refine the node into smaller tasks (nodes only, no dependencies).\n\
Return 2-6 child tasks. Use unique ids.\n\
Continue decomposition until leaf tasks are pure functions suitable as GPU kernel functions (no side effects; control flow lives in the DAG).\n\
Leaf tasks that write files must have node_type=render AND include file_write or apply_patch in required_capabilities.\n\
All other tasks must have node_type=analysis and must NOT include file_write or apply_patch.\n\
Workspace root: {}\nWorkspace entries: {}\nAction space: paths must be under workspace root.\n{}\n\nINPUT:\n{}",
        workspace_root.display(), workspace_listing, schema,
        serde_json::to_string_pretty(& input).unwrap_or_default()
    );
    DecomposeDecomposeRequest {
        prompt,
        input,
        schema,
        caps: caps.into_iter().map(|s| s.to_string()).collect(),
        phase: "decompose_node",
        log_path: log_dir.join(format!("decompose_{}.json", node.id)),
        min_tasks: None,
        min_tasks_message: None,
        retry_on_parse: false,
    }
}
pub fn validate_decompose_payload(
    payload: Value,
    request: &DecomposeDecomposeRequest,
) -> Result<DecomposeDecomposeOutput, DecomposeDecomposeRetry> {
    let mut output: DecomposeDecomposeOutput = match serde_json::from_value(
        payload.clone(),
    ) {
        Ok(v) => v,
        Err(e) => {
            if request.retry_on_parse {
                let retry_prompt = format!(
                    "Your response did not match the schema.\n\
Return exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{\n      \"id\": \"t1\",\n      \"description\": \"string\",\n      \"deps\": [],\n      \"required_capabilities\": [\"{}\"],\n      \"node_type\": \"analysis|render\",\n      \"priority\": 0,\n      \"budget\": 3,\n      \"reasoning_trace\": \"string\"\n    }}\n  ]\n}}\n\
Allowed capability values:\n{}\n\n\
Invalid response:\n{}\n\n\
Original input:\n{}",
                    "file_write", request.caps.join(", "), serde_json::to_string_pretty(&
                    payload).unwrap_or_default(), serde_json::to_string_pretty(& request
                    .input).unwrap_or_default()
                );
                return Err(DecomposeDecomposeRetry::Retry {
                    prompt: retry_prompt,
                });
            }
            return Err(DecomposeDecomposeRetry::Retry {
                prompt: format!("parse_error: {}", e),
            });
        }
    };
    if let Some(min_tasks) = request.min_tasks {
        if output.tasks.len() < min_tasks {
            let min_tasks_message = request.min_tasks_message.unwrap_or(min_tasks);
            let retry_prompt = format!(
                "Your response must include at least {} tasks.\n\
Return exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{\n      \"id\": \"t1\",\n      \"description\": \"string\",\n      \"deps\": [],\n      \"required_capabilities\": [\"{}\"],\n      \"node_type\": \"analysis|render\",\n      \"priority\": 0,\n      \"budget\": 3,\n      \"reasoning_trace\": \"string\"\n    }}\n  ]\n}}\n\
Allowed capability values:\n{}\n\n\
Invalid response:\n{}\n\n\
Original input:\n{}",
                min_tasks_message, "file_write", request.caps.join(", "),
                serde_json::to_string_pretty(& payload).unwrap_or_default(),
                serde_json::to_string_pretty(& request.input).unwrap_or_default()
            );
            return Err(DecomposeDecomposeRetry::Retry {
                prompt: retry_prompt,
            });
        }
    }
    decompose_normalize_output(&mut output);
    let has_render = output
        .tasks
        .iter()
        .any(|t| t.node_type == DecomposeNodeType::Render);
    if !has_render {
        let render_hint = format!(
            "Your decomposition has no render nodes. At least one task must have node_type=render \
and include file_write or apply_patch in required_capabilities.\n\
Return exactly one fenced ```json block and nothing else.\n\
Schema:\n\
{{\n  \"tasks\": [\n    {{\n      \"id\": \"t1\",\n      \"description\": \"string\",\n      \"deps\": [],\n      \
\"required_capabilities\": [\"file_write\"],\n      \"node_type\": \"render\"\n    }}\n  ]\n}}\n\
Allowed capability values:\n{}\n\nOriginal input:\n{}",
            request.caps.join(", "), serde_json::to_string_pretty(& request.input)
            .unwrap_or_default()
        );
        return Err(DecomposeDecomposeRetry::EnsureRender {
            prompt: render_hint,
            original: output,
        });
    }
    Ok(output)
}
pub fn parse_decompose_payload(payload: Value) -> Result<DecomposeDecomposeOutput> {
    let mut output: DecomposeDecomposeOutput = serde_json::from_value(payload)
        .context("D_g output did not match schema")?;
    decompose_normalize_output(&mut output);
    Ok(output)
}
pub fn decompose_merge_outputs(
    original: DecomposeDecomposeOutput,
    retry: DecomposeDecomposeOutput,
) -> DecomposeDecomposeOutput {
    let mut merged = original.tasks;
    for t in retry.tasks {
        if !merged.iter().any(|e| e.id == t.id) {
            merged.push(t);
        }
    }
    DecomposeDecomposeOutput {
        tasks: merged,
    }
}
pub fn decompose_write_payload_log(log_path: &Path, payload: &Value) {
    if let Ok(pretty) = serde_json::to_string_pretty(payload) {
        let _ = std::fs::write(log_path, pretty);
    }
}
fn normalize_task_node_type(
    node_type: DecomposeNodeType,
    caps: &[PipelineCapability],
    description: &str,
) -> DecomposeNodeType {
    let render_cap = caps
        .iter()
        .any(|c| {
            matches!(c, PipelineCapability::FileWrite | PipelineCapability::ApplyPatch)
        });
    if render_cap { DecomposeNodeType::Render } else { DecomposeNodeType::Analysis }
}
fn decompose_normalize_output(output: &mut DecomposeDecomposeOutput) {
    for t in &mut output.tasks {
        t.node_type = normalize_task_node_type(
            t.node_type,
            &t.required_capabilities,
            &t.description,
        );
        let has_verify = t
            .required_capabilities
            .iter()
            .any(|c| c.class() == super::capability::CapabilityMode::Verify);
        let has_observe = t
            .required_capabilities
            .iter()
            .any(|c| c.class() == super::capability::CapabilityMode::Observe);
        let has_mutate = t
            .required_capabilities
            .iter()
            .any(|c| c.class() == super::capability::CapabilityMode::Mutate);
        if t.node_type == DecomposeNodeType::Analysis && has_verify && !has_observe
            && !has_mutate
        {
            if !t.required_capabilities.contains(&PipelineCapability::FileRead) {
                t.required_capabilities.push(PipelineCapability::FileRead);
            }
        }
    }
}

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use super::act::{apply_write_deltas, apply_read_deltas, summarize_execution_deltas};
use super::capability::{PipelineCapability, CapabilityMode};
use super::config::CapabilityConfigCapabilityPolicy;
use super::console;
use super::dag::NodeAuthority;
use super::dag::{ContextSnapshotNode, NodeStatus, ExecutionGraph, ExecutionNode};
pub use super::endpoint_worker::{llm_worker_new_tabs, TabManagerHandle};
use super::llm::{
    llm_client_call_agent_json_with_retry_allow_mismatch,
    llm_client_call_agent_raw_with_retry_allow_mismatch,
};
use super::ExecutionDelta;
use crate::ws_server::WsBridge;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleNodeOutcome {
    pub node_id: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub status_update: Option<NodeStatus>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModuleExecNodeResult {
    pub id: String,
    #[serde(default)]
    pub deltas: Vec<ExecutionDelta>,
    #[serde(default)]
    pub rationale: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModuleExecOutput {
    #[serde(default)]
    pub results: Vec<ModuleExecNodeResult>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModuleVerifyUpdate {
    pub id: String,
    pub status: NodeStatus,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModuleVerifyOutput {
    #[serde(default)]
    pub updates: Vec<ModuleVerifyUpdate>,
}
#[derive(Debug, Clone)]
pub enum ModuleNodeCallResult {
    Mutate { node_id: String, output: ModuleExecOutput },
    Readonly { node_id: String, output: ModuleExecOutput },
    Verify { node_id: String, output: ModuleVerifyOutput },
}
pub async fn module_call_llm_raw_with_retry_allow_mismatch(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabManagerHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    max_retries: u32,
    delay_secs: u64,
) -> Result<String> {
    llm_client_call_agent_raw_with_retry_allow_mismatch(
            bridge,
            endpoint_id,
            url,
            stateful,
            prompt,
            role_schema,
            phase,
            node_id,
            tabs,
            max_tabs,
            tab_cooldown_ms,
            max_retries,
            delay_secs,
        )
        .await
}
pub async fn module_call_llm_json_with_retry_allow_mismatch(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabManagerHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    max_retries: u32,
    delay_secs: u64,
) -> Result<Value> {
    llm_client_call_agent_json_with_retry_allow_mismatch(
            bridge,
            endpoint_id,
            url,
            stateful,
            prompt,
            role_schema,
            phase,
            node_id,
            tabs,
            max_tabs,
            tab_cooldown_ms,
            max_retries,
            delay_secs,
        )
        .await
}
pub async fn module_init_io_workers(
    bridge: &WsBridge,
    config: &super::config::CapabilityConfig,
    tabs: &TabManagerHandle,
) {
    super::endpoint_worker::llm_worker_init_workers(bridge, config, tabs).await;
}
pub fn module_take_recovery_signal(log_root: &Path) -> Option<String> {
    let path = log_root.join("recovery_signal.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| v.get("reason").and_then(|r| r.as_str()).map(|s| s.to_string()))
}
pub struct ModuleNodeProcessReport {
    pub node_id: String,
    pub had_error: bool,
    pub repair_kind: Option<String>,
    pub repair_succeeded: bool,
}
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum ModuleDispatchMode {
    Mutate,
    Verify,
    Readonly,
}
const MUTATE_SCHEMA: &str = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"results\": [\n    { \"id\": \"t1\", \"deltas\": [ { \"type\": \"write_file\", \"path\": \"x\", \"content\": \"...\" } ], \"rationale\": \"string\" }\n  ]\n}\nAllowed delta types:\n- read_file { path }\n- list_dir { path }\n- read_command { command, args }\n- write_file { path, content }\n- replace_text { path, find, replace }\n- delete_file { path }\n";
const VERIFY_SCHEMA: &str = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"updates\": [\n    { \"id\": \"t1\", \"status\": \"completed\", \"error\": null }\n  ]\n}\nAllowed status values: pending, ready, running, completed, failed, blocked.\n";
const READONLY_SCHEMA: &str = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"results\": [\n    { \"id\": \"t1\", \"deltas\": [ { \"type\": \"read_file\", \"path\": \"x\" } ], \"rationale\": \"string\" }\n  ]\n}\nAllowed delta types: read_file, list_dir, read_command.\n";
struct ModuleModeConfig {
    phase: &'static str,
    schema: &'static str,
    log_name: fn(u64) -> String,
}
fn module_log_name_mutate(iter: u64) -> String {
    format!("iter_{:03}_execute_output.json", iter)
}
fn module_log_name_verify(iter: u64) -> String {
    format!("iter_{:03}_verify_output.json", iter)
}
fn module_log_name_readonly(iter: u64) -> String {
    format!("iter_{:03}_readonly_output.json", iter)
}
const MODE_CONFIGS: [ModuleModeConfig; 3] = [
    ModuleModeConfig {
        phase: "mutate",
        schema: MUTATE_SCHEMA,
        log_name: module_log_name_mutate,
    },
    ModuleModeConfig {
        phase: "verify",
        schema: VERIFY_SCHEMA,
        log_name: module_log_name_verify,
    },
    ModuleModeConfig {
        phase: "readonly",
        schema: READONLY_SCHEMA,
        log_name: module_log_name_readonly,
    },
];
type ModuleParseFn = fn(Value, &ExecutionNode, u64) -> Result<ModuleNodeCallResult>;
const PARSE_FNS: [ModuleParseFn; 3] = [
    module_parse_mutate,
    module_parse_verify,
    module_parse_readonly,
];
pub(crate) fn module_repair_node(
    graph: &mut ExecutionGraph,
    node_id: &str,
    policy: &CapabilityConfigCapabilityPolicy,
    repair_radius: usize,
    max_repairs: u32,
) -> Option<String> {
    let node = graph.get_node_mut(node_id)?;
    if node.repair_attempts >= max_repairs {
        return None;
    }
    node.repair_attempts += 1;
    drop(node);
    let rules: &[fn(
        &mut ExecutionGraph,
        &str,
        &CapabilityConfigCapabilityPolicy,
        usize,
    ) -> Option<&'static str>] = &[
        module_rule_retry,
        module_rule_capability_downgrade,
        module_rule_dependency_rewire,
        module_rule_node_split,
    ];
    for rule in rules {
        if let Some(kind) = rule(graph, node_id, policy, repair_radius) {
            return Some(kind.to_string());
        }
    }
    None
}
pub(crate) fn module_process_call_result(
    node_id: String,
    call_result: Result<ModuleNodeCallResult>,
    graph: &mut ExecutionGraph,
    cwd: &[PathBuf],
    max_output_lines: usize,
    log_root: &Path,
    iter: u64,
    policy: &CapabilityConfigCapabilityPolicy,
    repair_radius: usize,
    max_repairs: u32,
) -> Result<ModuleNodeProcessReport> {
    let outcome = call_result
        .and_then(|r| module_apply_node_result(
            r,
            graph,
            cwd,
            max_output_lines,
            log_root,
            iter,
            policy,
        ));
    if let Err(e) = outcome {
        if let Some(kind) = module_repair_node(
            graph,
            &node_id,
            policy,
            repair_radius,
            max_repairs,
        ) {
            return Ok(ModuleNodeProcessReport {
                node_id,
                had_error: true,
                repair_kind: Some(kind),
                repair_succeeded: true,
            });
        }
        let _ = graph.update_status(&node_id, NodeStatus::Failed);
        if let Some(n) = graph.get_node_mut(&node_id) {
            n.error = Some(e.to_string());
        }
        return Ok(ModuleNodeProcessReport {
            node_id,
            had_error: true,
            repair_kind: Some("repair_failed".to_string()),
            repair_succeeded: false,
        });
    }
    if let Some(n) = graph.get_node_mut(&node_id) {
        if n.readonly_fail_count > policy.max_node_retries {
            n.reasoning_trace = Some(
                format!(
                    "REWRITE_REQUESTED: readonly failures exceeded {}", policy
                    .max_node_retries
                ),
            );
            n.readonly_fail_count = 0;
            n.status = NodeStatus::Pending;
            n.error = None;
            n.result = None;
        }
    }
    Ok(ModuleNodeProcessReport {
        node_id,
        had_error: false,
        repair_kind: None,
        repair_succeeded: false,
    })
}
fn module_rule_retry(
    graph: &mut ExecutionGraph,
    node_id: &str,
    policy: &CapabilityConfigCapabilityPolicy,
    _repair_radius: usize,
) -> Option<&'static str> {
    let node = graph.get_node_mut(node_id)?;
    if node.readonly_fail_count < policy.max_node_retries {
        node.status = NodeStatus::Ready;
        node.error = None;
        node.result = None;
        return Some("retry");
    }
    None
}
fn module_rule_capability_downgrade(
    graph: &mut ExecutionGraph,
    node_id: &str,
    _policy: &CapabilityConfigCapabilityPolicy,
    _repair_radius: usize,
) -> Option<&'static str> {
    let node = graph.get_node_mut(node_id)?;
    if node.required_capabilities.iter().any(|c| c.class() == CapabilityMode::Mutate) {
        node.required_capabilities = vec![PipelineCapability::FileRead];
        node.status = NodeStatus::Pending;
        node.error = None;
        node.result = None;
        return Some("capability_downgrade");
    }
    None
}
fn module_rule_dependency_rewire(
    graph: &mut ExecutionGraph,
    node_id: &str,
    _policy: &CapabilityConfigCapabilityPolicy,
    repair_radius: usize,
) -> Option<&'static str> {
    if repair_radius < 1 {
        return None;
    }
    let failed_deps: HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| n.status == NodeStatus::Failed)
        .map(|n| n.id.clone())
        .collect();
    let node = graph.get_node_mut(node_id)?;
    let before = node.deps.len();
    node.deps.retain(|dep| !failed_deps.contains(dep));
    if node.deps.len() != before {
        node.status = NodeStatus::Pending;
        return Some("dependency_rewire");
    }
    None
}
fn module_rule_node_split(
    graph: &mut ExecutionGraph,
    node_id: &str,
    _policy: &CapabilityConfigCapabilityPolicy,
    _repair_radius: usize,
) -> Option<&'static str> {
    let node_snapshot = graph.get_node(node_id).cloned()?;
    if node_snapshot.description.len() <= 120 {
        return None;
    }
    let new_id = format!("{}_analysis", node_snapshot.id);
    if graph.get_node(&new_id).is_some() {
        return None;
    }
    let new_node = ExecutionNode {
        id: new_id.clone(),
        description: format!("Analyze: {}", node_snapshot.description),
        status: NodeStatus::Pending,
        deps: node_snapshot.deps.clone(),
        required_capabilities: vec![PipelineCapability::ReadDag],
        node_type: super::decompose::DecomposeNodeType::Analysis,
        priority: node_snapshot.priority,
        budget: node_snapshot.budget,
        reasoning_trace: None,
        result: None,
        error: None,
        readonly_fail_count: 0,
        repair_attempts: 0,
        completed_iter: None,
    };
    if let Some(node) = graph.get_node_mut(node_id) {
        node.deps = vec![new_id.clone()];
    }
    graph.nodes.push(new_node);
    graph.rebuild_index();
    Some("node_split")
}
fn module_parse_mutate(
    payload: Value,
    node: &ExecutionNode,
    iter: u64,
) -> Result<ModuleNodeCallResult> {
    let output = module_parse_exec_output(&payload, &node.id)?;
    if node.node_type != super::decompose::DecomposeNodeType::Render {
        return Err(anyhow::anyhow!("non-render node attempted mutation call"));
    }
    let delta_count: usize = output.results.iter().map(|r| r.deltas.len()).sum();
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"executor","results":{},"deltas":{}}}"#,
        iter, output.results.len(), delta_count
    );
    Ok(ModuleNodeCallResult::Mutate {
        node_id: node.id.clone(),
        output,
    })
}
fn module_parse_verify(
    payload: Value,
    node: &ExecutionNode,
    iter: u64,
) -> Result<ModuleNodeCallResult> {
    let output: ModuleVerifyOutput = serde_json::from_value(payload)
        .context("verifier output did not match schema")?;
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"verifier","updates":{}}}"#, iter, output
        .updates.len()
    );
    Ok(ModuleNodeCallResult::Verify {
        node_id: node.id.clone(),
        output,
    })
}
fn module_parse_readonly(
    payload: Value,
    node: &ExecutionNode,
    iter: u64,
) -> Result<ModuleNodeCallResult> {
    let output = module_parse_exec_output(&payload, &node.id)?;
    if output
        .results
        .iter()
        .any(|r| {
            r
                .deltas
                .iter()
                .any(|d| {
                    matches!(
                        d, ExecutionDelta::WriteFile { .. } | ExecutionDelta::ReplaceText
                        { .. } | ExecutionDelta::DeleteFile { .. }
                    )
                })
        })
    {
        return Err(anyhow::anyhow!("readonly node returned mutation deltas"));
    }
    let delta_count: usize = output.results.iter().map(|r| r.deltas.len()).sum();
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"readonly","results":{},"deltas":{}}}"#,
        iter, output.results.len(), delta_count
    );
    Ok(ModuleNodeCallResult::Readonly {
        node_id: node.id.clone(),
        output,
    })
}
type ModuleModePredicate = fn(&NodeAuthority) -> bool;
type ModuleModeValidator = fn(&NodeAuthority, &str) -> Result<()>;
struct ModuleModeRule {
    predicate: ModuleModePredicate,
    validate: ModuleModeValidator,
    mode: ModuleDispatchMode,
}
fn module_validate_verify(ctx: &NodeAuthority, node_id: &str) -> Result<()> {
    ctx.capabilities
        .iter()
        .any(|c| c.class() == CapabilityMode::Verify)
        .then_some(())
        .ok_or_else(|| {
            anyhow::anyhow!("node {} has no Verify-class capability", node_id)
        })
}
fn module_validate_mutate(ctx: &NodeAuthority, node_id: &str) -> Result<()> {
    ctx.capabilities
        .iter()
        .any(|c| c.class() == CapabilityMode::Mutate)
        .then_some(())
        .ok_or_else(|| {
            anyhow::anyhow!("node {} has no Mutate-class capability", node_id)
        })
}
fn module_validate_pass(_: &NodeAuthority, _: &str) -> Result<()> {
    Ok(())
}
const MODE_RULES: [ModuleModeRule; 3] = [
    ModuleModeRule {
        predicate: NodeAuthority::is_verify_context,
        validate: module_validate_verify,
        mode: ModuleDispatchMode::Verify,
    },
    ModuleModeRule {
        predicate: NodeAuthority::is_mutation_context,
        validate: module_validate_mutate,
        mode: ModuleDispatchMode::Mutate,
    },
    ModuleModeRule {
        predicate: |_| true,
        validate: module_validate_pass,
        mode: ModuleDispatchMode::Readonly,
    },
];
fn module_select_mode(ctx: &NodeAuthority, node_id: &str) -> Result<ModuleDispatchMode> {
    if ctx.is_verify_context() && !ctx.is_mutation_context()
        && ctx.capabilities.iter().any(|c| c.class() == CapabilityMode::Observe)
    {
        return Ok(ModuleDispatchMode::Readonly);
    }
    MODE_RULES
        .iter()
        .find(|r| (r.predicate)(ctx))
        .map(|r| (r.validate)(ctx, node_id).map(|_| r.mode))
        .unwrap_or(Ok(ModuleDispatchMode::Readonly))
}
pub async fn module_call_node(
    node: &ExecutionNode,
    ctx: &NodeAuthority,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    role_schema: &str,
    tabs: &TabManagerHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    workspace_root: &Path,
    context: &[ContextSnapshotNode],
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<ModuleNodeCallResult> {
    let mode = module_select_mode(ctx, &node.id)?;
    module_call_mode(
            mode,
            node,
            bridge,
            endpoint_id,
            url,
            stateful,
            role_schema,
            tabs,
            max_tabs,
            tab_cooldown_ms,
            workspace_root,
            context,
            log_dir,
            iter,
            retries,
            delay_secs,
        )
        .await
}
pub fn module_apply_node_result(
    result: ModuleNodeCallResult,
    graph: &mut ExecutionGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    policy: &CapabilityConfigCapabilityPolicy,
) -> Result<()> {
    match result {
        ModuleNodeCallResult::Mutate { node_id, output } => {
            module_apply_mutate_output(
                node_id,
                output,
                graph,
                roots,
                max_output_lines,
                log_dir,
                iter,
                policy,
            )
        }
        ModuleNodeCallResult::Readonly { node_id, output } => {
            module_apply_readonly_output(
                node_id,
                output,
                graph,
                roots,
                max_output_lines,
                log_dir,
                iter,
                policy.max_node_retries,
            )
        }
        ModuleNodeCallResult::Verify { node_id, output } => {
            module_apply_verify_output(node_id, output, graph, log_dir, iter)
        }
    }
}
/// Convenience: call + apply in one shot (used by dispatch path in mod.rs).
pub async fn module_dispatch_node(
    node: &ExecutionNode,
    ctx: &NodeAuthority,
    graph: &mut ExecutionGraph,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    role_schema: &str,
    tabs: &TabManagerHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    workspace_root: &Path,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<ModuleNodeOutcome> {
    let mode = module_select_mode(ctx, &node.id)?;
    let result = module_call_mode(
            mode,
            node,
            bridge,
            endpoint_id,
            url,
            stateful,
            role_schema,
            tabs,
            max_tabs,
            tab_cooldown_ms,
            workspace_root,
            &[],
            log_dir,
            iter,
            retries,
            delay_secs,
        )
        .await?;
    module_apply_node_result(
        result,
        graph,
        roots,
        max_output_lines,
        log_dir,
        iter,
        &CapabilityConfigCapabilityPolicy::default(),
    )?;
    Ok(ModuleNodeOutcome {
        node_id: node.id.clone(),
        result: None,
        error: None,
        status_update: None,
    })
}
async fn module_call_mode(
    mode: ModuleDispatchMode,
    node: &ExecutionNode,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    role_schema: &str,
    tabs: &TabManagerHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    workspace_root: &Path,
    context: &[ContextSnapshotNode],
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<ModuleNodeCallResult> {
    let config = &MODE_CONFIGS[mode as usize];
    let input: Value = match mode {
        ModuleDispatchMode::Mutate => {
            serde_json::json!(
                { "nodes" : [{ "id" : node.id, "description" : node.description,
                "node_type" : node.node_type, "deps" : node.deps, "required_capabilities"
                : node.required_capabilities }], "context" : context }
            )
        }
        ModuleDispatchMode::Verify => {
            serde_json::json!(
                { "nodes" : [{ "id" : node.id, "status" : node.status, "result" : node
                .result, "description" : node.description }], "context" : context }
            )
        }
        ModuleDispatchMode::Readonly => {
            serde_json::json!(
                { "node" : { "id" : node.id, "description" : node.description,
                "node_type" : node.node_type, "deps" : node.deps, "required_capabilities"
                : node.required_capabilities }, "context" : context }
            )
        }
    };
    let prompt = format!(
        "{}\n\nWorkspace root: {}\nContext radius: {} nodes.\nINPUT:\n{}", config.schema,
        workspace_root.display(), context.len(), serde_json::to_string_pretty(& input)
        .unwrap_or_default()
    );
    let payload = module_llm_call_with_retry(
            bridge,
            endpoint_id,
            url,
            stateful,
            &prompt,
            config.schema,
            &input,
            role_schema,
            config.phase,
            Some(&node.id),
            tabs,
            max_tabs,
            tab_cooldown_ms,
            retries,
            delay_secs,
        )
        .await?;
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(log_dir.join((config.log_name)(iter)), pretty);
    }
    let parse_fn = PARSE_FNS[mode as usize];
    parse_fn(payload, node, iter)
}
async fn module_llm_call_with_retry(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    schema: &str,
    input: &Value,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabManagerHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<Value> {
    let payload = llm_client_call_agent_json_with_retry_allow_mismatch(
            bridge,
            endpoint_id,
            url,
            stateful,
            prompt,
            role_schema,
            phase,
            node_id,
            tabs,
            max_tabs,
            tab_cooldown_ms,
            retries,
            delay_secs,
        )
        .await?;
    Ok(payload)
}
fn module_apply_mutate_output(
    node_id: String,
    output: ModuleExecOutput,
    graph: &mut ExecutionGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    _log_dir: &Path,
    iter: u64,
    policy: &CapabilityConfigCapabilityPolicy,
) -> Result<()> {
    if module_mutate_is_blocked(&node_id, graph, policy) {
        eprintln!(
            r#"[capability] {{"iter":{},"phase":"executor","event":"render_blocked","node":"{}"}}"#,
            iter, node_id
        );
        let _ = graph.update_status(&node_id, NodeStatus::Ready);
        return Ok(());
    }
    for mut result in output.results {
        module_coerce_id(&mut result.id, &node_id);
        module_apply_mutate_result(
            result,
            &node_id,
            graph,
            roots,
            max_output_lines,
            iter,
        );
    }
    Ok(())
}
fn module_apply_verify_output(
    node_id: String,
    output: ModuleVerifyOutput,
    graph: &mut ExecutionGraph,
    _log_dir: &Path,
    iter: u64,
) -> Result<()> {
    for mut upd in output.updates {
        module_coerce_id(&mut upd.id, &node_id);
        let _ = graph.update_status(&upd.id, upd.status);
        if let Some(n) = graph.get_node_mut(&upd.id) {
            n.error = upd.error;
            if upd.status == NodeStatus::Completed {
                n.completed_iter = Some(iter);
            }
        }
    }
    if let Some(node) = graph.get_node_mut(&node_id) {
        let has_mutate = node
            .required_capabilities
            .iter()
            .any(|c| c.class() == CapabilityMode::Mutate);
        let has_observe = node
            .required_capabilities
            .iter()
            .any(|c| c.class() == CapabilityMode::Observe);
        let has_verify = node
            .required_capabilities
            .iter()
            .any(|c| c.class() == CapabilityMode::Verify);
        if has_verify && !has_mutate && !has_observe && node.status == NodeStatus::Ready
        {
            let _ = graph.update_status(&node_id, NodeStatus::Completed);
            if let Some(n) = graph.get_node_mut(&node_id) {
                n.completed_iter = Some(iter);
            }
            eprintln!(
                "{}", console::console_ui_phase("verify", &
                format!("node={} auto-completed verify-only", node_id))
            );
        }
    }
    Ok(())
}
fn module_apply_readonly_output(
    node_id: String,
    output: ModuleExecOutput,
    graph: &mut ExecutionGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    max_node_retries: u32,
) -> Result<()> {
    output
        .results
        .is_empty()
        .then(|| module_log_empty_readonly(iter, &node_id, log_dir))
        .and_then(|_| graph.update_status(&node_id, NodeStatus::Ready).ok());
    for mut result in output.results {
        module_coerce_id(&mut result.id, &node_id);
        module_apply_readonly_result(
            result,
            &node_id,
            graph,
            roots,
            max_output_lines,
            log_dir,
            iter,
            max_node_retries,
        );
    }
    Ok(())
}
fn module_partition_deltas(
    deltas: Vec<ExecutionDelta>,
) -> (Vec<ExecutionDelta>, Vec<ExecutionDelta>) {
    deltas
        .into_iter()
        .partition(|d| {
            matches!(
                d, ExecutionDelta::ReadFile { .. } | ExecutionDelta::ListDir { .. } |
                ExecutionDelta::ReadCommand { .. }
            )
        })
}
fn module_mutate_is_blocked(
    node_id: &str,
    graph: &ExecutionGraph,
    policy: &CapabilityConfigCapabilityPolicy,
) -> bool {
    let not_render = graph
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| n.node_type != super::decompose::DecomposeNodeType::Render)
        .unwrap_or(false);
    let render_blocked = policy.require_final_render
        && graph
            .nodes
            .iter()
            .any(|n| {
                n.status != NodeStatus::Completed
                    && !n
                        .required_capabilities
                        .iter()
                        .any(|c| c.class() == CapabilityMode::Mutate)
            });
    not_render || render_blocked
}
fn module_apply_mutate_result(
    result: ModuleExecNodeResult,
    node_id: &str,
    graph: &mut ExecutionGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    iter: u64,
) {
    if !result.rationale.trim().is_empty() {
        eprintln!(
            "{}", console::console_ui_phase("mutate", & format!("node={} rationale={}",
            node_id, console::console_ui_truncate(& result.rationale, 180)))
        );
    }
    let (ro, mutate) = module_partition_deltas(result.deltas);
    if !ro.is_empty() {
        let _ = apply_read_deltas(&ro, roots, max_output_lines);
    }
    let (out, _, err) = apply_write_deltas(&mutate, roots, roots, max_output_lines);
    let _ = graph.update_status(node_id, NodeStatus::Running);
    let (requires_verify, has_err) = if let Some(n) = graph.get_node_mut(node_id) {
        n.result = Some(out);
        n.error = err;
        (
            n.required_capabilities.iter().any(|c| c.class() == CapabilityMode::Verify),
            n.error.is_some(),
        )
    } else {
        (false, false)
    };
    let final_status = requires_verify
        .then_some(None)
        .unwrap_or_else(|| Some(
            if has_err { NodeStatus::Failed } else { NodeStatus::Completed },
        ));
    if let Some(s) = final_status {
        let _ = graph.update_status(node_id, s);
        if s == NodeStatus::Completed {
            if let Some(n) = graph.get_node_mut(node_id) {
                n.completed_iter = Some(iter);
            }
        }
    }
}
fn module_log_empty_readonly(iter: u64, node_id: &str, log_dir: &Path) {
    let summary = serde_json::json!(
        { "iter" : iter, "phase" : "readonly", "event" : "empty_results", "node" :
        node_id }
    );
    let _ = std::fs::write(
        log_dir.join(format!("iter_{:03}_readonly_error.json", iter)),
        serde_json::to_string_pretty(&summary).unwrap_or_default(),
    );
    eprintln!("[capability] {}", summary);
}
fn module_log_readonly_error(
    iter: u64,
    node_id: &str,
    err: &str,
    deltas: &[ExecutionDelta],
    log_dir: &Path,
) {
    let summary = serde_json::json!(
        { "iter" : iter, "phase" : "readonly", "event" : "delta_error", "node" : node_id,
        "error" : err, "deltas" : summarize_execution_deltas(deltas) }
    );
    let _ = std::fs::write(
        log_dir.join(format!("iter_{:03}_readonly_error.json", iter)),
        serde_json::to_string_pretty(&summary).unwrap_or_default(),
    );
    eprintln!("[capability] {}", summary);
}
fn module_apply_readonly_result(
    result: ModuleExecNodeResult,
    node_id: &str,
    graph: &mut ExecutionGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    max_node_retries: u32,
) {
    if !result.rationale.trim().is_empty() {
        eprintln!(
            "{}", console::console_ui_phase("observe", & format!("node={} rationale={}",
            node_id, console::console_ui_truncate(& result.rationale, 180)))
        );
    }
    let (ro, mutate) = module_partition_deltas(result.deltas);
    if !mutate.is_empty() {
        let msg = "read-only context received mutation deltas".to_string();
        if let Some(n) = graph.get_node_mut(node_id) {
            n.result = Some(msg.clone());
            n.error = Some(msg);
        }
        let _ = graph.update_status(node_id, NodeStatus::Failed);
        return;
    }
    let (out, _, err) = apply_read_deltas(&ro, roots, max_output_lines);
    let _ = graph.update_status(node_id, NodeStatus::Running);
    if let Some(n) = graph.get_node_mut(node_id) {
        n.result = Some(out);
        n.error = err.clone();
    }
    let effective_budget = graph
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .and_then(|n| n.budget)
        .unwrap_or(max_node_retries);
    let next_status = err
        .map(|e| {
            module_log_readonly_error(iter, node_id, &e, &ro, log_dir);
            let fail_count = graph
                .get_node_mut(node_id)
                .map(|n| {
                    n.readonly_fail_count += 1;
                    n.readonly_fail_count
                })
                .unwrap_or(1);
            if fail_count >= effective_budget {
                NodeStatus::Failed
            } else {
                NodeStatus::Ready
            }
        })
        .unwrap_or(NodeStatus::Completed);
    let _ = graph.update_status(node_id, next_status);
    if next_status == NodeStatus::Completed {
        if let Some(n) = graph.get_node_mut(node_id) {
            n.completed_iter = Some(iter);
        }
    }
}
fn module_coerce_id(result_id: &mut String, canonical: &str) {
    result_id.clear();
    result_id.push_str(canonical);
}
fn module_parse_exec_output(
    payload: &Value,
    default_id: &str,
) -> Result<ModuleExecOutput> {
    if let Ok(v) = serde_json::from_value::<ModuleExecOutput>(payload.clone()) {
        return Ok(v);
    }
    if let Some(deltas) = payload.get("deltas") {
        let deltas: Vec<ExecutionDelta> = serde_json::from_value(deltas.clone())
            .context("executor deltas field invalid")?;
        let rationale = payload
            .get("rationale")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let id = payload
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or(default_id)
            .to_string();
        return Ok(ModuleExecOutput {
            results: vec![ModuleExecNodeResult { id, deltas, rationale }],
        });
    }
    Err(anyhow::anyhow!("executor output did not match schema"))
}

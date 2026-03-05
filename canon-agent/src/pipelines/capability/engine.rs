use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::collections::HashSet;

use super::act::{apply_mutations, apply_read_only, summarize_deltas};
use super::config::CapabilityPolicy;
use super::dag::AuthorityContext;
use super::capability::{Capability, CapabilityClass};
use super::console;
use super::dag::{ContextNode, Status, TaskGraph, TaskNode};
use super::llm::call_agent_json_with_retry_allow_mismatch;
use super::tab_management::TabsHandle;
use super::Delta;
use crate::ws_server::WsBridge;

// ── Shared output types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeOutcome {
    pub node_id: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub status_update: Option<Status>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VerifyUpdate {
    pub id: String,
    pub status: Status,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VerifyOutput {
    #[serde(default)]
    pub updates: Vec<VerifyUpdate>,
}

#[derive(Debug, Clone)]
pub enum NodeCallResult {
    Mutate { node_id: String, output: ExecOutput },
    Readonly { node_id: String, output: ExecOutput },
    Verify { node_id: String, output: VerifyOutput },
}

pub struct NodeProcessReport {
    pub node_id: String,
    pub had_error: bool,
    pub repair_kind: Option<String>,
    pub repair_succeeded: bool,
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum DispatchMode { Mutate, Verify, Readonly }

const MUTATE_SCHEMA: &str = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"results\": [\n    { \"id\": \"t1\", \"deltas\": [ { \"type\": \"write_file\", \"path\": \"x\", \"content\": \"...\" } ], \"rationale\": \"string\" }\n  ]\n}\nAllowed delta types:\n- read_file { path }\n- list_dir { path }\n- read_command { command, args }\n- write_file { path, content }\n- replace_text { path, find, replace }\n- delete_file { path }\n";
const VERIFY_SCHEMA: &str = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"updates\": [\n    { \"id\": \"t1\", \"status\": \"completed\", \"error\": null }\n  ]\n}\nAllowed status values: pending, ready, running, completed, failed, blocked.\n";
const READONLY_SCHEMA: &str = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"results\": [\n    { \"id\": \"t1\", \"deltas\": [ { \"type\": \"read_file\", \"path\": \"x\" } ], \"rationale\": \"string\" }\n  ]\n}\nAllowed delta types: read_file, list_dir, read_command.\n";

struct ModeConfig {
    phase: &'static str,
    schema: &'static str,
    log_name: fn(u64) -> String,
}

fn log_name_mutate(iter: u64) -> String {
    format!("iter_{:03}_execute_output.json", iter)
}

fn log_name_verify(iter: u64) -> String {
    format!("iter_{:03}_verify_output.json", iter)
}

fn log_name_readonly(iter: u64) -> String {
    format!("iter_{:03}_readonly_output.json", iter)
}

const MODE_CONFIGS: [ModeConfig; 3] = [
    ModeConfig { phase: "mutate", schema: MUTATE_SCHEMA, log_name: log_name_mutate },
    ModeConfig { phase: "verify", schema: VERIFY_SCHEMA, log_name: log_name_verify },
    ModeConfig { phase: "readonly", schema: READONLY_SCHEMA, log_name: log_name_readonly },
];

type ParseFn = fn(Value, &TaskNode, u64) -> Result<NodeCallResult>;

const PARSE_FNS: [ParseFn; 3] = [parse_mutate, parse_verify, parse_readonly];

pub(crate) fn repair_node(
    graph: &mut TaskGraph,
    node_id: &str,
    policy: &CapabilityPolicy,
    repair_radius: usize,
    max_repairs: u32,
) -> Option<String> {
    let node = graph.get_node_mut(node_id)?;
    if node.repair_attempts >= max_repairs {
        return None;
    }
    node.repair_attempts += 1;
    drop(node);

    let rules: &[fn(&mut TaskGraph, &str, &CapabilityPolicy, usize) -> Option<&'static str>] = &[
        rule_retry,
        rule_capability_downgrade,
        rule_dependency_rewire,
        rule_node_split,
    ];

    for rule in rules {
        if let Some(kind) = rule(graph, node_id, policy, repair_radius) {
            return Some(kind.to_string());
        }
    }

    None
}

pub(crate) fn process_call_result(
    node_id: String,
    call_result: Result<NodeCallResult>,
    graph: &mut TaskGraph,
    cwd: &[PathBuf],
    max_output_lines: usize,
    log_root: &Path,
    iter: u64,
    policy: &CapabilityPolicy,
    repair_radius: usize,
    max_repairs: u32,
) -> Result<NodeProcessReport> {
    let outcome = call_result.and_then(|r| apply_node_result(
        r,
        graph,
        cwd,
        max_output_lines,
        log_root,
        iter,
        policy,
    ));
    if let Err(e) = outcome {
        if let Some(kind) = repair_node(graph, &node_id, policy, repair_radius, max_repairs) {
            return Ok(NodeProcessReport {
                node_id,
                had_error: true,
                repair_kind: Some(kind),
                repair_succeeded: true,
            });
        }
        let _ = graph.update_status(&node_id, Status::Failed);
        if let Some(n) = graph.get_node_mut(&node_id) {
            n.error = Some(e.to_string());
        }
        return Ok(NodeProcessReport {
            node_id,
            had_error: true,
            repair_kind: Some("repair_failed".to_string()),
            repair_succeeded: false,
        });
    }

    if let Some(n) = graph.get_node_mut(&node_id) {
        if n.readonly_fail_count > policy.max_node_retries {
            n.reasoning_trace = Some(format!(
                "REWRITE_REQUESTED: readonly failures exceeded {}",
                policy.max_node_retries
            ));
            n.readonly_fail_count = 0;
            n.status = Status::Pending;
            n.error = None;
            n.result = None;
        }
    }

    Ok(NodeProcessReport {
        node_id,
        had_error: false,
        repair_kind: None,
        repair_succeeded: false,
    })
}

fn rule_retry(
    graph: &mut TaskGraph,
    node_id: &str,
    policy: &CapabilityPolicy,
    _repair_radius: usize,
) -> Option<&'static str> {
    let node = graph.get_node_mut(node_id)?;
    if node.readonly_fail_count < policy.max_node_retries {
        node.status = Status::Ready;
        node.error = None;
        node.result = None;
        return Some("retry");
    }
    None
}

fn rule_capability_downgrade(
    graph: &mut TaskGraph,
    node_id: &str,
    _policy: &CapabilityPolicy,
    _repair_radius: usize,
) -> Option<&'static str> {
    let node = graph.get_node_mut(node_id)?;
    if node
        .required_capabilities
        .iter()
        .any(|c| c.class() == CapabilityClass::Mutate)
    {
        node.required_capabilities = vec![Capability::FileRead];
        node.status = Status::Pending;
        node.error = None;
        node.result = None;
        return Some("capability_downgrade");
    }
    None
}

fn rule_dependency_rewire(
    graph: &mut TaskGraph,
    node_id: &str,
    _policy: &CapabilityPolicy,
    repair_radius: usize,
) -> Option<&'static str> {
    if repair_radius < 1 {
        return None;
    }
    let failed_deps: HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| n.status == Status::Failed)
        .map(|n| n.id.clone())
        .collect();
    let node = graph.get_node_mut(node_id)?;
    let before = node.deps.len();
    node.deps.retain(|dep| !failed_deps.contains(dep));
    if node.deps.len() != before {
        node.status = Status::Pending;
        return Some("dependency_rewire");
    }
    None
}

fn rule_node_split(
    graph: &mut TaskGraph,
    node_id: &str,
    _policy: &CapabilityPolicy,
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
    let new_node = TaskNode {
        id: new_id.clone(),
        description: format!("Analyze: {}", node_snapshot.description),
        status: Status::Pending,
        deps: node_snapshot.deps.clone(),
        required_capabilities: vec![Capability::ReadDag],
        node_type: super::decompose::NodeType::Analysis,
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

fn parse_mutate(payload: Value, node: &TaskNode, iter: u64) -> Result<NodeCallResult> {
    let output = parse_exec_output(&payload, &node.id)?;
    if node.node_type != super::decompose::NodeType::Render {
        return Err(anyhow::anyhow!("non-render node attempted mutation call"));
    }
    let delta_count: usize = output.results.iter().map(|r| r.deltas.len()).sum();
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"executor","results":{},"deltas":{}}}"#,
        iter,
        output.results.len(),
        delta_count
    );
    Ok(NodeCallResult::Mutate { node_id: node.id.clone(), output })
}

fn parse_verify(payload: Value, node: &TaskNode, iter: u64) -> Result<NodeCallResult> {
    let output: VerifyOutput = serde_json::from_value(payload)
        .context("verifier output did not match schema")?;
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"verifier","updates":{}}}"#,
        iter,
        output.updates.len()
    );
    Ok(NodeCallResult::Verify { node_id: node.id.clone(), output })
}

fn parse_readonly(payload: Value, node: &TaskNode, iter: u64) -> Result<NodeCallResult> {
    let output = parse_exec_output(&payload, &node.id)?;
    if output.results.iter().any(|r| r.deltas.iter().any(|d|
        matches!(d, Delta::WriteFile {..} | Delta::ReplaceText {..} | Delta::DeleteFile {..})))
    {
        return Err(anyhow::anyhow!("readonly node returned mutation deltas"));
    }
    let delta_count: usize = output.results.iter().map(|r| r.deltas.len()).sum();
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"readonly","results":{},"deltas":{}}}"#,
        iter,
        output.results.len(),
        delta_count
    );
    Ok(NodeCallResult::Readonly { node_id: node.id.clone(), output })
}

// ── Mode selection ────────────────────────────────────────────────────────────

type ModePredicate = fn(&AuthorityContext) -> bool;
type ModeValidator = fn(&AuthorityContext, &str) -> Result<()>;

struct ModeRule {
    predicate: ModePredicate,
    validate: ModeValidator,
    mode: DispatchMode,
}

fn validate_verify(ctx: &AuthorityContext, node_id: &str) -> Result<()> {
    ctx.capabilities.iter()
        .any(|c| c.class() == CapabilityClass::Verify)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("node {} has no Verify-class capability", node_id))
}

fn validate_mutate(ctx: &AuthorityContext, node_id: &str) -> Result<()> {
    ctx.capabilities.iter()
        .any(|c| c.class() == CapabilityClass::Mutate)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("node {} has no Mutate-class capability", node_id))
}

fn validate_pass(_: &AuthorityContext, _: &str) -> Result<()> { Ok(()) }

const MODE_RULES: [ModeRule; 3] = [
    ModeRule { predicate: AuthorityContext::is_verify_context, validate: validate_verify, mode: DispatchMode::Verify },
    ModeRule { predicate: AuthorityContext::is_mutation_context, validate: validate_mutate, mode: DispatchMode::Mutate },
    ModeRule { predicate: |_| true, validate: validate_pass, mode: DispatchMode::Readonly },
];

fn select_mode(ctx: &AuthorityContext, node_id: &str) -> Result<DispatchMode> {
    if ctx.is_verify_context()
        && !ctx.is_mutation_context()
        && ctx.capabilities.iter().any(|c| c.class() == CapabilityClass::Observe)
    {
        return Ok(DispatchMode::Readonly);
    }
    MODE_RULES.iter()
        .find(|r| (r.predicate)(ctx))
        .map(|r| (r.validate)(ctx, node_id).map(|_| r.mode))
        .unwrap_or(Ok(DispatchMode::Readonly))
}

// ── Public entry points ───────────────────────────────────────────────────────

pub async fn call_node(
    node: &TaskNode,
    ctx: &AuthorityContext,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    role_schema: &str,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    workspace_root: &Path,
    context: &[ContextNode],
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<NodeCallResult> {
    let mode = select_mode(ctx, &node.id)?;
    call_mode(
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

pub fn apply_node_result(
    result: NodeCallResult,
    graph: &mut TaskGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    policy: &CapabilityPolicy,
) -> Result<()> {
    match result {
        NodeCallResult::Mutate { node_id, output } =>
            apply_mutate_output(node_id, output, graph, roots, max_output_lines, log_dir, iter, policy),
        NodeCallResult::Readonly { node_id, output } =>
            apply_readonly_output(node_id, output, graph, roots, max_output_lines, log_dir, iter, policy.max_node_retries),
        NodeCallResult::Verify { node_id, output } =>
            apply_verify_output(node_id, output, graph, log_dir, iter),
    }
}

/// Convenience: call + apply in one shot (used by dispatch path in mod.rs).
pub async fn dispatch_node(
    node: &TaskNode,
    ctx: &AuthorityContext,
    graph: &mut TaskGraph,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    role_schema: &str,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    workspace_root: &Path,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<NodeOutcome> {
    let mode = select_mode(ctx, &node.id)?;
    let result = call_mode(
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
    apply_node_result(result, graph, roots, max_output_lines, log_dir, iter,
                      &CapabilityPolicy::default())?;
    Ok(NodeOutcome { node_id: node.id.clone(), result: None, error: None, status_update: None })
}

// ── Single LLM call dispatcher ────────────────────────────────────────────────

async fn call_mode(
    mode: DispatchMode,
    node: &TaskNode,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    role_schema: &str,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    workspace_root: &Path,
    context: &[ContextNode],
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<NodeCallResult> {
    let config = &MODE_CONFIGS[mode as usize];
    let input: Value = match mode {
        DispatchMode::Mutate => serde_json::json!({
            "nodes": [{"id": node.id, "description": node.description,
                       "node_type": node.node_type, "deps": node.deps,
                       "required_capabilities": node.required_capabilities}],
            "context": context
        }),
        DispatchMode::Verify => serde_json::json!({
            "nodes": [{"id": node.id, "status": node.status,
                       "result": node.result, "description": node.description}],
            "context": context
        }),
        DispatchMode::Readonly => serde_json::json!({
            "node": {"id": node.id, "description": node.description,
                     "node_type": node.node_type, "deps": node.deps,
                     "required_capabilities": node.required_capabilities},
            "context": context
        }),
    };

    let prompt = format!(
        "{}\n\nWorkspace root: {}\nContext radius: {} nodes.\nINPUT:\n{}",
        config.schema, workspace_root.display(), context.len(),
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );

    // ── LLM call with one schema-retry ────────────────────────────────────────
    let payload = llm_call_with_retry(
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

    // ── Log ───────────────────────────────────────────────────────────────────
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(log_dir.join((config.log_name)(iter)), pretty);
    }

    // ── Parse and wrap ────────────────────────────────────────────────────────
    let parse_fn = PARSE_FNS[mode as usize];
    parse_fn(payload, node, iter)
}

// ── Shared LLM call with one schema-retry ─────────────────────────────────────

async fn llm_call_with_retry(
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
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<Value> {
    let payload = call_agent_json_with_retry_allow_mismatch(
        bridge, endpoint_id, url, stateful, prompt, role_schema, phase, node_id,
        tabs, max_tabs, tab_cooldown_ms, retries, delay_secs,
    ).await?;
    Ok(payload)
}

// ── Apply outputs (legitimately distinct per mode) ────────────────────────────

fn apply_mutate_output(
    node_id: String,
    output: ExecOutput,
    graph: &mut TaskGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    _log_dir: &Path,
    iter: u64,
    policy: &CapabilityPolicy,
) -> Result<()> {
    if mutate_is_blocked(&node_id, graph, policy) {
        eprintln!(r#"[capability] {{"iter":{},"phase":"executor","event":"render_blocked","node":"{}"}}"#, iter, node_id);
        let _ = graph.update_status(&node_id, Status::Ready);
        return Ok(());
    }
    for mut result in output.results {
        coerce_id(&mut result.id, &node_id);
        apply_mutate_result(result, &node_id, graph, roots, max_output_lines, iter);
    }
    Ok(())
}

fn apply_verify_output(
    node_id: String,
    output: VerifyOutput,
    graph: &mut TaskGraph,
    _log_dir: &Path,
    iter: u64,
) -> Result<()> {
    for mut upd in output.updates {
        coerce_id(&mut upd.id, &node_id);
        let _ = graph.update_status(&upd.id, upd.status);
        if let Some(n) = graph.get_node_mut(&upd.id) {
            n.error = upd.error;
            if upd.status == Status::Completed {
                n.completed_iter = Some(iter);
            }
        }
    }
    if let Some(node) = graph.get_node_mut(&node_id) {
        let has_mutate = node.required_capabilities.iter().any(|c| c.class() == CapabilityClass::Mutate);
        let has_observe = node.required_capabilities.iter().any(|c| c.class() == CapabilityClass::Observe);
        let has_verify = node.required_capabilities.iter().any(|c| c.class() == CapabilityClass::Verify);
        if has_verify && !has_mutate && !has_observe && node.status == Status::Ready {
            let _ = graph.update_status(&node_id, Status::Completed);
            if let Some(n) = graph.get_node_mut(&node_id) {
                n.completed_iter = Some(iter);
            }
            eprintln!(
                "{}",
                console::phase("verify", &format!("node={} auto-completed verify-only", node_id))
            );
        }
    }
    Ok(())
}

fn apply_readonly_output(
    node_id: String,
    output: ExecOutput,
    graph: &mut TaskGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    max_node_retries: u32,
) -> Result<()> {
    output.results.is_empty()
        .then(|| log_empty_readonly(iter, &node_id, log_dir))
        .and_then(|_| graph.update_status(&node_id, Status::Ready).ok());
    for mut result in output.results {
        coerce_id(&mut result.id, &node_id);
        apply_readonly_result(result, &node_id, graph, roots, max_output_lines, log_dir, iter, max_node_retries);
    }
    Ok(())
}

fn partition_deltas(deltas: Vec<Delta>) -> (Vec<Delta>, Vec<Delta>) {
    deltas.into_iter()
        .partition(|d| matches!(d, Delta::ReadFile {..} | Delta::ListDir {..} | Delta::ReadCommand {..}))
}

fn mutate_is_blocked(node_id: &str, graph: &TaskGraph, policy: &CapabilityPolicy) -> bool {
    let not_render = graph.nodes.iter()
        .find(|n| n.id == node_id)
        .map(|n| n.node_type != super::decompose::NodeType::Render)
        .unwrap_or(false);
    let render_blocked = policy.require_final_render && graph.nodes.iter().any(|n| {
        n.status != Status::Completed
            && !n.required_capabilities.iter().any(|c| c.class() == CapabilityClass::Mutate)
    });
    not_render || render_blocked
}

fn apply_mutate_result(
    result: ExecNodeResult,
    node_id: &str,
    graph: &mut TaskGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    iter: u64,
) {
    if !result.rationale.trim().is_empty() {
        eprintln!(
            "{}",
            console::phase(
                "mutate",
                &format!("node={} rationale={}", node_id, console::truncate(&result.rationale, 180))
            )
        );
    }
    let (ro, mutate) = partition_deltas(result.deltas);
    if !ro.is_empty() {
        let _ = apply_read_only(&ro, roots, max_output_lines);
    }
    let (out, _, err) = apply_mutations(&mutate, roots, roots, max_output_lines);
    let _ = graph.update_status(node_id, Status::Running);
    let (requires_verify, has_err) = if let Some(n) = graph.get_node_mut(node_id) {
        n.result = Some(out);
        n.error = err;
        (n.required_capabilities.iter().any(|c| c.class() == CapabilityClass::Verify), n.error.is_some())
    } else {
        (false, false)
    };
    let final_status = requires_verify
        .then_some(None)
        .unwrap_or_else(|| Some(if has_err { Status::Failed } else { Status::Completed }));
    if let Some(s) = final_status {
        let _ = graph.update_status(node_id, s);
        if s == Status::Completed {
            if let Some(n) = graph.get_node_mut(node_id) {
                n.completed_iter = Some(iter);
            }
        }
    }
}

fn log_empty_readonly(iter: u64, node_id: &str, log_dir: &Path) {
    let summary = serde_json::json!({"iter": iter, "phase": "readonly", "event": "empty_results", "node": node_id});
    let _ = std::fs::write(log_dir.join(format!("iter_{:03}_readonly_error.json", iter)),
                           serde_json::to_string_pretty(&summary).unwrap_or_default());
    eprintln!("[capability] {}", summary);
}

fn log_readonly_error(iter: u64, node_id: &str, err: &str, deltas: &[Delta], log_dir: &Path) {
    let summary = serde_json::json!({"iter": iter, "phase": "readonly", "event": "delta_error",
                                     "node": node_id, "error": err, "deltas": summarize_deltas(deltas)});
    let _ = std::fs::write(log_dir.join(format!("iter_{:03}_readonly_error.json", iter)),
                           serde_json::to_string_pretty(&summary).unwrap_or_default());
    eprintln!("[capability] {}", summary);
}

fn apply_readonly_result(
    result: ExecNodeResult,
    node_id: &str,
    graph: &mut TaskGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    max_node_retries: u32,
) {
    if !result.rationale.trim().is_empty() {
        eprintln!(
            "{}",
            console::phase(
                "observe",
                &format!("node={} rationale={}", node_id, console::truncate(&result.rationale, 180))
            )
        );
    }
    let (ro, mutate) = partition_deltas(result.deltas);
    if !mutate.is_empty() {
        let msg = "read-only context received mutation deltas".to_string();
        if let Some(n) = graph.get_node_mut(node_id) { n.result = Some(msg.clone()); n.error = Some(msg); }
        let _ = graph.update_status(node_id, Status::Failed);
        return;
    }

    let (out, _, err) = apply_read_only(&ro, roots, max_output_lines);
    let _ = graph.update_status(node_id, Status::Running);
    if let Some(n) = graph.get_node_mut(node_id) { n.result = Some(out); n.error = err.clone(); }

    let effective_budget = graph.nodes.iter()
        .find(|n| n.id == node_id)
        .and_then(|n| n.budget)
        .unwrap_or(max_node_retries);
    let next_status = err.map(|e| {
        log_readonly_error(iter, node_id, &e, &ro, log_dir);
        let fail_count = graph.get_node_mut(node_id)
            .map(|n| { n.readonly_fail_count += 1; n.readonly_fail_count })
            .unwrap_or(1);
        if fail_count >= effective_budget { Status::Failed } else { Status::Ready }
    }).unwrap_or(Status::Completed);

    let _ = graph.update_status(node_id, next_status);
    if next_status == Status::Completed {
        if let Some(n) = graph.get_node_mut(node_id) {
            n.completed_iter = Some(iter);
        }
    }
}

fn coerce_id(result_id: &mut String, canonical: &str) {
    result_id.clear();
    result_id.push_str(canonical);
}

// ── Exec output parser ────────────────────────────────────────────────────────

fn parse_exec_output(payload: &Value, default_id: &str) -> Result<ExecOutput> {
    if let Ok(v) = serde_json::from_value::<ExecOutput>(payload.clone()) {
        return Ok(v);
    }
    if let Some(deltas) = payload.get("deltas") {
        let deltas: Vec<Delta> = serde_json::from_value(deltas.clone())
            .context("executor deltas field invalid")?;
        let rationale = payload.get("rationale").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let id = payload.get("id").and_then(|v| v.as_str()).unwrap_or(default_id).to_string();
        return Ok(ExecOutput { results: vec![ExecNodeResult { id, deltas, rationale }] });
    }
    Err(anyhow::anyhow!("executor output did not match schema"))
}

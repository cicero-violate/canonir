use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::act::{apply_mutations, apply_read_only, summarize_deltas};
use super::config::CapabilityPolicy;
use super::dag::AuthorityContext;
use super::capability::Capability;
use super::dag::{Status, TaskGraph, TaskNode};
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

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum DispatchMode { Mutate, Verify, Readonly }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextNode {
    pub id: String,
    pub description: String,
    pub node_type: super::decompose::NodeType,
    pub deps: Vec<String>,
    pub required_capabilities: Vec<Capability>,
    pub status: Status,
    pub result: Option<String>,
    pub error: Option<String>,
}

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

fn select_mode(ctx: &AuthorityContext, node_id: &str) -> Result<DispatchMode> {
    if ctx.is_verify_context() {
        ctx.require(Capability::StatusUpdateOnly).map_err(|e| anyhow::anyhow!(e))?;
        return Ok(DispatchMode::Verify);
    }
    if ctx.is_mutation_context() {
        if !(ctx.has(Capability::FileWrite) || ctx.has(Capability::ApplyPatch)) {
            return Err(anyhow::anyhow!("node {} missing capability FileWrite or ApplyPatch", node_id));
        }
        return Ok(DispatchMode::Mutate);
    }
    Ok(DispatchMode::Readonly)
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
    let idx = result_index(&result);
    APPLY_FNS[idx](result, graph, roots, max_output_lines, log_dir, iter, policy)
}

type ApplyFn = fn(NodeCallResult, &mut TaskGraph, &[PathBuf], usize, &Path, u64, &CapabilityPolicy) -> Result<()>;

const APPLY_FNS: [ApplyFn; 3] = [apply_mutate_wrapper, apply_readonly_wrapper, apply_verify_wrapper];

fn result_index(result: &NodeCallResult) -> usize {
    match result {
        NodeCallResult::Mutate { .. } => 0,
        NodeCallResult::Readonly { .. } => 1,
        NodeCallResult::Verify { .. } => 2,
    }
}

fn apply_mutate_wrapper(
    result: NodeCallResult,
    graph: &mut TaskGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    policy: &CapabilityPolicy,
) -> Result<()> {
    if let NodeCallResult::Mutate { node_id, output } = result {
        apply_mutate_output(node_id, output, graph, roots, max_output_lines, log_dir, iter, policy)
    } else {
        unreachable!("apply_mutate_wrapper received wrong variant")
    }
}

fn apply_readonly_wrapper(
    result: NodeCallResult,
    graph: &mut TaskGraph,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    policy: &CapabilityPolicy,
) -> Result<()> {
    if let NodeCallResult::Readonly { node_id, output } = result {
        apply_readonly_output(node_id, output, graph, roots, max_output_lines, log_dir, iter, policy.max_node_retries)
    } else {
        unreachable!("apply_readonly_wrapper received wrong variant")
    }
}

fn apply_verify_wrapper(
    result: NodeCallResult,
    graph: &mut TaskGraph,
    _roots: &[PathBuf],
    _max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    _policy: &CapabilityPolicy,
) -> Result<()> {
    if let NodeCallResult::Verify { node_id, output } = result {
        apply_verify_output(node_id, output, graph, log_dir, iter)
    } else {
        unreachable!("apply_verify_wrapper received wrong variant")
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
    if let Some(node) = graph.get_node(&node_id) {
        if node.node_type != super::decompose::NodeType::Render {
            eprintln!(r#"[capability] {{"iter":{},"phase":"executor","event":"non_render_mutation","node":"{}"}}"#, iter, node_id);
            let _ = graph.update_status(&node_id, Status::Ready);
            return Ok(());
        }
    }
    if policy.require_final_render {
        let non_mutation_pending = graph.nodes.iter().any(|n| {
            n.status != Status::Completed
                && !n.required_capabilities.iter().any(|c| matches!(c, Capability::FileWrite | Capability::ApplyPatch))
        });
        if non_mutation_pending {
            eprintln!(r#"[capability] {{"iter":{},"phase":"executor","event":"render_blocked","node":"{}"}}"#, iter, node_id);
            let _ = graph.update_status(&node_id, Status::Ready);
            return Ok(());
        }
    }
    for mut result in output.results {
        coerce_id(&mut result.id, &node_id);
        let (ro, mutate): (Vec<Delta>, Vec<Delta>) = result.deltas.into_iter()
            .partition(|d| matches!(d, Delta::ReadFile {..} | Delta::ListDir {..} | Delta::ReadCommand {..}));
        if !ro.is_empty() { let _ = apply_read_only(&ro, roots, max_output_lines); }
        let (out, _, err) = apply_mutations(&mutate, roots, roots, max_output_lines);
        let _ = graph.update_status(&result.id, Status::Running);
        let (requires_verify, has_err) = if let Some(n) = graph.get_node_mut(&result.id) {
            n.result = Some(out);
            n.error = err;
            (n.required_capabilities.contains(&Capability::StatusUpdateOnly), n.error.is_some())
        } else {
            (false, false)
        };
        if !requires_verify {
            let s = if has_err { Status::Failed } else { Status::Completed };
            let _ = graph.update_status(&result.id, s);
        }
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
        if let Some(n) = graph.get_node_mut(&upd.id) { n.error = upd.error; }
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
    if output.results.is_empty() {
        let summary = serde_json::json!({"iter": iter, "phase": "readonly", "event": "empty_results", "node": node_id});
        let _ = std::fs::write(log_dir.join(format!("iter_{:03}_readonly_error.json", iter)),
                               serde_json::to_string_pretty(&summary).unwrap_or_default());
        eprintln!("[capability] {}", summary);
        let _ = graph.update_status(&node_id, Status::Ready);
        return Ok(());
    }
    for mut result in output.results {
        coerce_id(&mut result.id, &node_id);
        let (ro, mutate): (Vec<Delta>, Vec<Delta>) = result.deltas.into_iter()
            .partition(|d| matches!(d, Delta::ReadFile {..} | Delta::ListDir {..} | Delta::ReadCommand {..}));
        if !mutate.is_empty() {
            let msg = "read-only context received mutation deltas".to_string();
            if let Some(n) = graph.get_node_mut(&result.id) { n.result = Some(msg.clone()); n.error = Some(msg); }
            let _ = graph.update_status(&result.id, Status::Failed);
            continue;
        }
        let (out, _, err) = apply_read_only(&ro, roots, max_output_lines);
        let _ = graph.update_status(&result.id, Status::Running);
        if let Some(n) = graph.get_node_mut(&result.id) { n.result = Some(out); n.error = err.clone(); }
        if err.is_some() {
            let summary = serde_json::json!({"iter": iter, "phase": "readonly", "event": "delta_error",
                                             "node": result.id, "error": err, "deltas": summarize_deltas(&ro)});
            let _ = std::fs::write(log_dir.join(format!("iter_{:03}_readonly_error.json", iter)),
                                   serde_json::to_string_pretty(&summary).unwrap_or_default());
            eprintln!("[capability] {}", summary);
            let fail_count = if let Some(n) = graph.get_node_mut(&result.id) {
                n.readonly_fail_count += 1;
                n.readonly_fail_count
            } else {
                1
            };
            if fail_count >= max_node_retries {
                eprintln!(
                    r#"[capability] {{"iter":{},"phase":"readonly","event":"escalate_failed","node":"{}","fail_count":{}}}"#,
                    iter, result.id, fail_count
                );
                let _ = graph.update_status(&result.id, Status::Failed);
            } else {
                let _ = graph.update_status(&result.id, Status::Ready);
            }
        } else {
            let _ = graph.update_status(&result.id, Status::Completed);
        }
    }
    Ok(())
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

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::act::{apply_mutations, apply_read_only, summarize_deltas};
use super::config::CapabilityPolicy;
use super::dag::AuthorityContext;
use super::capability::Capability;
use super::dag::{Status, TaskGraph, TaskNode};
use super::llm::call_agent_json_with_retry;
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
enum DispatchMode { Mutate, Readonly, Verify }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextNode {
    pub id: String,
    pub description: String,
    pub node_type: super::decompose::NodeType,
    pub deps: Vec<String>,
    pub required_capabilities: Vec<Capability>,
    pub status: Status,
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
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::tab_management::TabSlots>,
    reuse_tabs: bool,
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
    call_mode(mode, node, bridge, endpoint_id, url, role_schema, tabs, reuse_tabs,
              max_tabs, tab_cooldown_ms, workspace_root, context, log_dir, iter, retries, delay_secs).await
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
        NodeCallResult::Mutate   { node_id, output } => apply_mutate_output(node_id, output, graph, roots, max_output_lines, log_dir, iter, policy),
        NodeCallResult::Readonly { node_id, output } => apply_readonly_output(node_id, output, graph, roots, max_output_lines, log_dir, iter),
        NodeCallResult::Verify   { node_id, output } => apply_verify_output(node_id, output, graph, log_dir, iter),
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
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::tab_management::TabSlots>,
    reuse_tabs: bool,
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
    let result = call_mode(mode, node, bridge, endpoint_id, url, role_schema, tabs,
                           reuse_tabs, max_tabs, tab_cooldown_ms, workspace_root,
                           &[], log_dir, iter, retries, delay_secs).await?;
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
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::tab_management::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    workspace_root: &Path,
    context: &[ContextNode],
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<NodeCallResult> {
    // ── Build mode-specific prompt pieces ─────────────────────────────────────
    let (phase, schema, input, log_name): (&str, &str, Value, String) = match mode {
        DispatchMode::Mutate => (
            "mutate",
            "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"results\": [\n    { \"id\": \"t1\", \"deltas\": [ { \"type\": \"write_file\", \"path\": \"x\", \"content\": \"...\" } ], \"rationale\": \"string\" }\n  ]\n}\nAllowed delta types:\n- read_file { path }\n- list_dir { path }\n- read_command { command, args }\n- write_file { path, content }\n- replace_text { path, find, replace }\n- delete_file { path }\n",
            serde_json::json!({
                "nodes": [{"id": node.id, "description": node.description,
                           "node_type": node.node_type, "deps": node.deps,
                           "required_capabilities": node.required_capabilities}],
                "context": context
            }),
            format!("iter_{:03}_execute_output.json", iter),
        ),
        DispatchMode::Verify => (
            "verify",
            "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"updates\": [\n    { \"id\": \"t1\", \"status\": \"completed\", \"error\": null }\n  ]\n}\nAllowed status values: pending, ready, running, completed, failed, blocked.\n",
            serde_json::json!({
                "nodes": [{"id": node.id, "status": node.status,
                           "result": node.result, "description": node.description}],
                "context": context
            }),
            format!("iter_{:03}_verify_output.json", iter),
        ),
        DispatchMode::Readonly => (
            "readonly",
            "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"results\": [\n    { \"id\": \"t1\", \"deltas\": [ { \"type\": \"read_file\", \"path\": \"x\" } ], \"rationale\": \"string\" }\n  ]\n}\nAllowed delta types: read_file, list_dir, read_command.\n",
            serde_json::json!({
                "node": {"id": node.id, "description": node.description,
                         "node_type": node.node_type, "deps": node.deps,
                         "required_capabilities": node.required_capabilities},
                "context": context
            }),
            format!("iter_{:03}_readonly_output.json", iter),
        ),
    };

    let prompt = format!(
        "{}\n\nWorkspace root: {}\nContext radius: {} nodes.\nINPUT:\n{}",
        schema, workspace_root.display(), context.len(),
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );

    // ── LLM call with one schema-retry ────────────────────────────────────────
    let payload = llm_call_with_retry(
        bridge, endpoint_id, url, &prompt, schema, &input, role_schema, phase,
        tabs, reuse_tabs, max_tabs, tab_cooldown_ms, retries, delay_secs,
    ).await?;

    // ── Log ───────────────────────────────────────────────────────────────────
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(log_dir.join(&log_name), pretty);
    }

    // ── Parse and wrap ────────────────────────────────────────────────────────
    match mode {
        DispatchMode::Mutate => {
            let output = parse_exec_output(&payload, &node.id)?;
            if node.node_type != super::decompose::NodeType::Render {
                return Err(anyhow::anyhow!("non-render node attempted mutation call"));
            }
            let delta_count: usize = output.results.iter().map(|r| r.deltas.len()).sum();
            eprintln!(r#"[capability] {{"iter":{},"phase":"executor","results":{},"deltas":{}}}"#,
                      iter, output.results.len(), delta_count);
            Ok(NodeCallResult::Mutate { node_id: node.id.clone(), output })
        }
        DispatchMode::Verify => {
            let output: VerifyOutput = serde_json::from_value(payload)
                .context("verifier output did not match schema")?;
            eprintln!(r#"[capability] {{"iter":{},"phase":"verifier","updates":{}}}"#,
                      iter, output.updates.len());
            Ok(NodeCallResult::Verify { node_id: node.id.clone(), output })
        }
        DispatchMode::Readonly => {
            let output = parse_exec_output(&payload, &node.id)?;
            // Guard: readonly must not contain mutation deltas.
            if output.results.iter().any(|r| r.deltas.iter().any(|d|
                matches!(d, Delta::WriteFile {..} | Delta::ReplaceText {..} | Delta::DeleteFile {..})))
            {
                return Err(anyhow::anyhow!("readonly node returned mutation deltas"));
            }
            let delta_count: usize = output.results.iter().map(|r| r.deltas.len()).sum();
            eprintln!(r#"[capability] {{"iter":{},"phase":"readonly","results":{},"deltas":{}}}"#,
                      iter, output.results.len(), delta_count);
            Ok(NodeCallResult::Readonly { node_id: node.id.clone(), output })
        }
    }
}

// ── Shared LLM call with one schema-retry ─────────────────────────────────────

async fn llm_call_with_retry(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    prompt: &str,
    schema: &str,
    input: &Value,
    role_schema: &str,
    phase: &str,
    tabs: &tokio::sync::Mutex<super::tab_management::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<Value> {
    let payload = call_agent_json_with_retry(
        bridge, endpoint_id, url, prompt, role_schema, phase,
        tabs, reuse_tabs, max_tabs, tab_cooldown_ms, retries, delay_secs,
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
    if let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) {
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
        if result.id != node_id { result.id = node_id.clone(); }
        let (ro, mutate): (Vec<Delta>, Vec<Delta>) = result.deltas.into_iter()
            .partition(|d| matches!(d, Delta::ReadFile {..} | Delta::ListDir {..} | Delta::ReadCommand {..}));
        if !ro.is_empty() { let _ = apply_read_only(&ro, roots, max_output_lines); }
        let (out, _, err) = apply_mutations(&mutate, roots, roots, max_output_lines);
        let _ = graph.update_status(&result.id, Status::Running);
        if let Some(n) = graph.get_node_mut(&result.id) { n.result = Some(out); n.error = err; }
        let requires_verify = graph.nodes.iter().find(|n| n.id == result.id)
            .map(|n| n.required_capabilities.contains(&Capability::StatusUpdateOnly))
            .unwrap_or(false);
        if !requires_verify {
            let has_err = graph.nodes.iter().find(|n| n.id == result.id)
                .and_then(|n| n.error.as_ref()).is_some();
            let _ = if has_err { graph.update_status(&result.id, Status::Failed) }
                    else { graph.update_status(&result.id, Status::Completed) };
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
        if upd.id != node_id {
            eprintln!(r#"[capability] {{"iter":{},"phase":"verifier","event":"id_mismatch","got":"{}","expected":"{}"}}"#,
                      iter, upd.id, node_id);
            upd.id = node_id.clone();
        }
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
        if result.id != node_id { result.id = node_id.clone(); }
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
            let _ = graph.update_status(&result.id, Status::Ready);
        } else {
            let _ = graph.update_status(&result.id, Status::Completed);
        }
    }
    Ok(())
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

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::act::{apply_mutations, apply_read_only, summarize_deltas};
use super::policy::CapabilityPolicy;
use super::authority::AuthorityContext;
use super::capability::Capability;
use super::dag::{Status, TaskGraph, TaskNode};
use super::llm::call_agent_json_with_retry;
use super::Delta;
use crate::ws_server::WsBridge;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextNode {
    pub id: String,
    pub description: String,
    pub node_type: super::decompose::NodeType,
    pub deps: Vec<String>,
    pub required_capabilities: Vec<Capability>,
    pub status: Status,
}

pub async fn call_node(
    node: &TaskNode,
    ctx: &AuthorityContext,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::llm::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    workspace_root: &Path,
    context: &[ContextNode],
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<NodeCallResult> {
    if ctx.is_verify_context() {
        ctx.require(Capability::StatusUpdateOnly).map_err(|e| anyhow::anyhow!(e))?;
        let output = call_verify(node, bridge, endpoint_id, url, role_schema, tabs, reuse_tabs, max_tabs, workspace_root, context, log_dir, iter, retries, delay_secs).await?;
        return Ok(NodeCallResult::Verify { node_id: node.id.clone(), output });
    }
    if ctx.is_mutation_context() {
        if !(ctx.has(Capability::FileWrite) || ctx.has(Capability::ApplyPatch)) {
            return Err(anyhow::anyhow!("node {} missing capability FileWrite or ApplyPatch", node.id));
        }
        let output = call_mutate(node, bridge, endpoint_id, url, role_schema, tabs, reuse_tabs, max_tabs, workspace_root, context, log_dir, iter, retries, delay_secs).await?;
        return Ok(NodeCallResult::Mutate { node_id: node.id.clone(), output });
    }
    let output = call_readonly(node, bridge, endpoint_id, url, role_schema, tabs, reuse_tabs, max_tabs, workspace_root, context, log_dir, iter, retries, delay_secs).await?;
    Ok(NodeCallResult::Readonly { node_id: node.id.clone(), output })
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
        NodeCallResult::Mutate { node_id, output } => apply_mutate_output(node_id, output, graph, roots, max_output_lines, log_dir, iter, policy),
        NodeCallResult::Readonly { node_id, output } => apply_readonly_output(node_id, output, graph, roots, max_output_lines, log_dir, iter),
        NodeCallResult::Verify { node_id, output } => apply_verify_output(node_id, output, graph, log_dir, iter),
    }
}

pub async fn dispatch_node(
    node: &TaskNode,
    ctx: &AuthorityContext,
    graph: &mut TaskGraph,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::llm::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    workspace_root: &Path,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<NodeOutcome> {
    if ctx.is_verify_context() {
        ctx.require(Capability::StatusUpdateOnly).map_err(|e| anyhow::anyhow!(e))?;
        return dispatch_verify(node, graph, bridge, endpoint_id, url, role_schema, tabs, reuse_tabs, max_tabs, workspace_root, log_dir, iter, retries, delay_secs).await;
    }
    if ctx.is_mutation_context() {
        if !(ctx.has(Capability::FileWrite) || ctx.has(Capability::ApplyPatch)) {
            return Err(anyhow::anyhow!("node {} missing capability FileWrite or ApplyPatch", node.id));
        }
        return dispatch_mutate(node, graph, bridge, endpoint_id, url, role_schema, tabs, reuse_tabs, max_tabs, workspace_root, roots, max_output_lines, log_dir, iter, retries, delay_secs).await;
    }
    dispatch_readonly(node, graph, bridge, endpoint_id, url, role_schema, tabs, reuse_tabs, max_tabs, workspace_root, roots, max_output_lines, log_dir, iter, retries, delay_secs).await
}

async fn call_mutate(
    node: &TaskNode,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::llm::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    workspace_root: &Path,
    context: &[ContextNode],
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<ExecOutput> {
    let input = serde_json::json!({
        "nodes": [{"id": node.id, "description": node.description, "node_type": node.node_type, "deps": node.deps, "required_capabilities": node.required_capabilities}],
        "context": context
    });
    let schema = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"results\": [\n    { \"id\": \"t1\", \"deltas\": [ { \"type\": \"write_file\", \"path\": \"x\", \"content\": \"...\" } ], \"rationale\": \"string\" }\n  ]\n}\nAllowed delta types:\n- read_file { path }\n- list_dir { path }\n- read_command { command, args }\n- write_file { path, content }\n- replace_text { path, find, replace }\n- delete_file { path }\n";
    let prompt = format!(
        "{}\n\nPropose deltas for node.\nWorkspace root: {}\nAction space: paths must be under workspace root.\nContext radius: {} nodes.\nINPUT:\n{}",
        schema,
        workspace_root.display(),
        context.len(),
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );
    let mut payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &prompt, role_schema, "mutate", tabs, reuse_tabs, max_tabs, retries, delay_secs).await?;
    let output: ExecOutput = match parse_exec_output(&payload, &node.id) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = format!(
                "Your response did not match the schema.\n{}\n\nInvalid response:\n{}\n\nOriginal input:\n{}",
                schema,
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
                serde_json::to_string_pretty(&input).unwrap_or_default()
            );
            let retry_payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &retry_prompt, role_schema, "mutate", tabs, reuse_tabs, max_tabs, 1, delay_secs).await?;
            payload = retry_payload.clone();
            parse_exec_output(&retry_payload, &node.id)?
        }
    };

    if node.node_type != super::decompose::NodeType::Render {
        return Err(anyhow::anyhow!("non-render node attempted mutation call"));
    }

    let exec_path = log_dir.join(format!("iter_{:03}_execute_output.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(exec_path, pretty);
    }
    let delta_count: usize = output.results.iter().map(|r| r.deltas.len()).sum();
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"executor","results":{},"deltas":{}}}"#,
        iter,
        output.results.len(),
        delta_count
    );
    Ok(output)
}

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
            eprintln!(
                r#"[capability] {{"iter":{},"phase":"executor","event":"non_render_mutation","node":"{}"}}"#,
                iter,
                node_id
            );
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
            eprintln!(
                r#"[capability] {{"iter":{},"phase":"executor","event":"render_blocked","node":"{}"}}"#,
                iter,
                node_id
            );
            let _ = graph.update_status(&node_id, Status::Ready);
            return Ok(());
        }
    }

    for mut result in output.results {
        if result.id != node_id {
            eprintln!(
                r#"[capability] {{"iter":{},"phase":"executor","event":"id_mismatch","got":"{}","expected":"{}"}}"#,
                iter,
                result.id,
                node_id
            );
            result.id = node_id.clone();
        }
        let (read_only, mutate): (Vec<Delta>, Vec<Delta>) = result
            .deltas
            .into_iter()
            .partition(|d| matches!(d, Delta::ReadFile { .. } | Delta::ListDir { .. } | Delta::ReadCommand { .. }));

        if !read_only.is_empty() {
            let _ = apply_read_only(&read_only, roots, max_output_lines);
        }

        let (out, _results, err) = apply_mutations(&mutate, roots, roots, max_output_lines);
        let _ = graph.update_status(&result.id, Status::Running);
        if let Some(n) = graph.get_node_mut(&result.id) {
            n.result = Some(out);
            n.error = err;
        }
        let requires_verify = graph
            .nodes
            .iter()
            .find(|n| n.id == result.id)
            .map(|n| n.required_capabilities.contains(&Capability::StatusUpdateOnly))
            .unwrap_or(false);
        if !requires_verify {
            let has_err = graph
                .nodes
                .iter()
                .find(|n| n.id == result.id)
                .and_then(|n| n.error.as_ref())
                .is_some();
            let _ = if has_err { graph.update_status(&result.id, Status::Failed) } else { graph.update_status(&result.id, Status::Completed) };
        }
    }
    Ok(())
}

async fn call_verify(
    node: &TaskNode,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::llm::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    workspace_root: &Path,
    context: &[ContextNode],
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<VerifyOutput> {
    let input = serde_json::json!({
        "nodes": [{"id": node.id, "status": node.status, "result": node.result, "description": node.description}],
        "context": context
    });
    let schema = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"updates\": [\n    { \"id\": \"t1\", \"status\": \"completed\", \"error\": null }\n  ]\n}\nAllowed status values: pending, ready, running, completed, failed, blocked.\n";
    let prompt = format!(
        "{}\n\nVerify node and return status update.\nWorkspace root: {}\nAction space: status updates only.\nContext radius: {} nodes.\nINPUT:\n{}",
        schema,
        workspace_root.display(),
        context.len(),
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );
    let mut payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &prompt, role_schema, "verify", tabs, reuse_tabs, max_tabs, retries, delay_secs).await?;
    let output: VerifyOutput = match serde_json::from_value(payload.clone()) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = format!(
                "Your response did not match the schema.\n{}\n\nInvalid response:\n{}\n\nOriginal input:\n{}",
                schema,
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
                serde_json::to_string_pretty(&input).unwrap_or_default()
            );
            let retry_payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &retry_prompt, role_schema, "verify", tabs, reuse_tabs, max_tabs, 1, delay_secs).await?;
            payload = retry_payload.clone();
            serde_json::from_value(retry_payload.clone()).context("verifier output did not match schema")?
        }
    };

    let verify_path = log_dir.join(format!("iter_{:03}_verify_output.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(verify_path, pretty);
    }
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"verifier","updates":{}}}"#,
        iter,
        output.updates.len()
    );
    Ok(output)
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
            eprintln!(
                r#"[capability] {{"iter":{},"phase":"verifier","event":"id_mismatch","got":"{}","expected":"{}"}}"#,
                iter,
                upd.id,
                node_id
            );
            upd.id = node_id.clone();
        }
        let _ = graph.update_status(&upd.id, upd.status);
        if let Some(n) = graph.get_node_mut(&upd.id) {
            n.error = upd.error;
        }
    }
    Ok(())
}

async fn call_readonly(
    node: &TaskNode,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::llm::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    workspace_root: &Path,
    context: &[ContextNode],
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<ExecOutput> {
    let input = serde_json::json!({
        "node": {"id": node.id, "description": node.description, "node_type": node.node_type, "deps": node.deps, "required_capabilities": node.required_capabilities},
        "context": context
    });
    let schema = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"results\": [\n    { \"id\": \"t1\", \"deltas\": [ { \"type\": \"read_file\", \"path\": \"x\" } ], \"rationale\": \"string\" }\n  ]\n}\nAllowed delta types: read_file, list_dir, read_command.\n";
    let prompt = format!(
        "{}\n\nRead-only node execution.\nWorkspace root: {}\nAction space: paths must be under workspace root.\nContext radius: {} nodes.\nINPUT:\n{}",
        schema,
        workspace_root.display(),
        context.len(),
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );
    let mut payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &prompt, role_schema, "readonly", tabs, reuse_tabs, max_tabs, retries, delay_secs).await?;
    let output: ExecOutput = match parse_exec_output(&payload, &node.id) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = format!(
                "Your response did not match the schema.\n{}\n\nInvalid response:\n{}\n\nOriginal input:\n{}",
                schema,
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
                serde_json::to_string_pretty(&input).unwrap_or_default()
            );
            let retry_payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &retry_prompt, role_schema, "readonly", tabs, reuse_tabs, max_tabs, 1, delay_secs).await?;
            payload = retry_payload.clone();
            parse_exec_output(&retry_payload, &node.id)?
        }
    };

    if output.results.iter().any(|r| r.deltas.iter().any(|d| matches!(d, Delta::WriteFile { .. } | Delta::ReplaceText { .. } | Delta::DeleteFile { .. }))) {
        let retry_prompt = format!(
            "Your response included mutation deltas in a read-only node. This is not allowed.\n\
Return exactly one fenced ```json block and nothing else.\n\
Allowed delta types: read_file, list_dir, read_command.\n\
Invalid response:\n{}\n\nOriginal input:\n{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
            serde_json::to_string_pretty(&input).unwrap_or_default()
        );
        let retry_payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &retry_prompt, role_schema, "verify", tabs, reuse_tabs, max_tabs, 1, delay_secs).await?;
        let retry_output = parse_exec_output(&retry_payload, &node.id)?;
        return Ok(retry_output);
    }

    let ro_path = log_dir.join(format!("iter_{:03}_readonly_output.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(ro_path, pretty);
    }
    let delta_count: usize = output.results.iter().map(|r| r.deltas.len()).sum();
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"readonly","results":{},"deltas":{}}}"#,
        iter,
        output.results.len(),
        delta_count
    );
    Ok(output)
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
        let summary = serde_json::json!({
            "iter": iter,
            "phase": "readonly",
            "event": "empty_results",
            "node": node_id,
        });
        let _ = std::fs::write(
            log_dir.join(format!("iter_{:03}_readonly_error.json", iter)),
            serde_json::to_string_pretty(&summary).unwrap_or_default(),
        );
        eprintln!("[capability] {}", summary);
        let _ = graph.update_status(&node_id, Status::Ready);
        return Ok(());
    }

    for mut result in output.results {
        if result.id != node_id {
            eprintln!(
                r#"[capability] {{"iter":{},"phase":"readonly","event":"id_mismatch","got":"{}","expected":"{}"}}"#,
                iter,
                result.id,
                node_id
            );
            result.id = node_id.clone();
        }
        let (read_only, mutate): (Vec<Delta>, Vec<Delta>) = result
            .deltas
            .into_iter()
            .partition(|d| matches!(d, Delta::ReadFile { .. } | Delta::ListDir { .. } | Delta::ReadCommand { .. }));
        if !mutate.is_empty() {
            let msg = "read-only context received mutation deltas".to_string();
            if let Some(n) = graph.get_node_mut(&result.id) {
                n.result = Some(msg.clone());
                n.error = Some(msg.clone());
            }
            let _ = graph.update_status(&result.id, Status::Failed);
            continue;
        }
        let (out, _results, err) = apply_read_only(&read_only, roots, max_output_lines);
        let _ = graph.update_status(&result.id, Status::Running);
        if let Some(n) = graph.get_node_mut(&result.id) {
            n.result = Some(out);
            n.error = err.clone();
        }
        let has_err = err.is_some();
        if has_err {
            let summary = serde_json::json!({
                "iter": iter,
                "phase": "readonly",
                "event": "delta_error",
                "node": result.id,
                "error": err,
                "deltas": summarize_deltas(&read_only),
            });
            let _ = std::fs::write(
                log_dir.join(format!("iter_{:03}_readonly_error.json", iter)),
                serde_json::to_string_pretty(&summary).unwrap_or_default(),
            );
            eprintln!("[capability] {}", summary);
            // Read-only failures are retryable; do not fail the node.
            let _ = graph.update_status(&result.id, Status::Ready);
        } else {
            let _ = graph.update_status(&result.id, Status::Completed);
        }
    }
    Ok(())
}
async fn dispatch_mutate(
    node: &TaskNode,
    graph: &mut TaskGraph,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::llm::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    workspace_root: &Path,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<NodeOutcome> {
    let input = serde_json::json!({ "nodes": [{"id": node.id, "description": node.description}] });
    let schema = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"results\": [\n    { \"id\": \"t1\", \"deltas\": [ { \"type\": \"write_file\", \"path\": \"x\", \"content\": \"...\" } ], \"rationale\": \"string\" }\n  ]\n}\nAllowed delta types:\n- read_file { path }\n- list_dir { path }\n- read_command { command, args }\n- write_file { path, content }\n- replace_text { path, find, replace }\n- delete_file { path }\n";
    let prompt = format!(
        "{}\n\nPropose deltas for node.\nWorkspace root: {}\nAction space: paths must be under workspace root.\nINPUT:\n{}",
        schema,
        workspace_root.display(),
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );
    let mut payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &prompt, role_schema, "readonly", tabs, reuse_tabs, max_tabs, retries, delay_secs).await?;
    let output: ExecOutput = match parse_exec_output(&payload, &node.id) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = format!(
                "Your response did not match the schema.\n{}\n\nInvalid response:\n{}\n\nOriginal input:\n{}",
                schema,
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
                serde_json::to_string_pretty(&input).unwrap_or_default()
            );
            let retry_payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &retry_prompt, role_schema, "readonly", tabs, reuse_tabs, max_tabs, 1, delay_secs).await?;
            payload = retry_payload.clone();
            parse_exec_output(&retry_payload, &node.id)?
        }
    };

    let exec_path = log_dir.join(format!("iter_{:03}_execute_output.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(exec_path, pretty);
    }
    let delta_count: usize = output.results.iter().map(|r| r.deltas.len()).sum();
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"executor","results":{},"deltas":{}}}"#,
        iter,
        output.results.len(),
        delta_count
    );

    if output.results.is_empty() {
        let summary = serde_json::json!({
            "iter": iter,
            "phase": "readonly",
            "event": "empty_results",
            "node": node.id,
        });
        let _ = std::fs::write(
            log_dir.join(format!("iter_{:03}_readonly_error.json", iter)),
            serde_json::to_string_pretty(&summary).unwrap_or_default(),
        );
        eprintln!("[capability] {}", summary);
        let _ = graph.update_status(&node.id, Status::Ready);
        return Ok(NodeOutcome { node_id: node.id.clone(), result: None, error: None, status_update: None });
    }

    for mut result in output.results {
        if result.id != node.id {
            eprintln!(
                r#"[capability] {{"iter":{},"phase":"executor","event":"id_mismatch","got":"{}","expected":"{}"}}"#,
                iter,
                result.id,
                node.id
            );
            result.id = node.id.clone();
        }
        let (read_only, mutate): (Vec<Delta>, Vec<Delta>) = result
            .deltas
            .into_iter()
            .partition(|d| matches!(d, Delta::ReadFile { .. } | Delta::ListDir { .. } | Delta::ReadCommand { .. }));

        if !read_only.is_empty() {
            let _ = apply_read_only(&read_only, roots, max_output_lines);
        }

        let (out, _results, err) = apply_mutations(&mutate, roots, roots, max_output_lines);
        let _ = graph.update_status(&result.id, Status::Running);
        if let Some(n) = graph.get_node_mut(&result.id) {
            n.result = Some(out);
            n.error = err;
        }
        let requires_verify = graph
            .nodes
            .iter()
            .find(|n| n.id == result.id)
            .map(|n| n.required_capabilities.contains(&Capability::StatusUpdateOnly))
            .unwrap_or(false);
        if !requires_verify {
            let has_err = graph
                .nodes
                .iter()
                .find(|n| n.id == result.id)
                .and_then(|n| n.error.as_ref())
                .is_some();
            let _ = if has_err { graph.update_status(&result.id, Status::Failed) } else { graph.update_status(&result.id, Status::Completed) };
        }
    }

    Ok(NodeOutcome { node_id: node.id.clone(), result: None, error: None, status_update: None })
}

async fn dispatch_verify(
    node: &TaskNode,
    graph: &mut TaskGraph,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::llm::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    workspace_root: &Path,
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<NodeOutcome> {
    let input = serde_json::json!({ "nodes": [{"id": node.id, "status": node.status, "result": node.result, "description": node.description}] });
    let schema = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"updates\": [\n    { \"id\": \"t1\", \"status\": \"completed\", \"error\": null }\n  ]\n}\nAllowed status values: pending, ready, running, completed, failed, blocked.\n";
    let prompt = format!(
        "{}\n\nVerify node and return status update.\nWorkspace root: {}\nAction space: status updates only.\nINPUT:\n{}",
        schema,
        workspace_root.display(),
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );
    let mut payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &prompt, role_schema, "readonly", tabs, reuse_tabs, max_tabs, retries, delay_secs).await?;
    let output: VerifyOutput = match serde_json::from_value(payload.clone()) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = format!(
                "Your response did not match the schema.\n{}\n\nInvalid response:\n{}\n\nOriginal input:\n{}",
                schema,
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
                serde_json::to_string_pretty(&input).unwrap_or_default()
            );
            let retry_payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &retry_prompt, role_schema, "readonly", tabs, reuse_tabs, max_tabs, 1, delay_secs).await?;
            payload = retry_payload.clone();
            serde_json::from_value(retry_payload.clone()).context("verifier output did not match schema")?
        }
    };

    let verify_path = log_dir.join(format!("iter_{:03}_verify_output.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(verify_path, pretty);
    }
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"verifier","updates":{}}}"#,
        iter,
        output.updates.len()
    );

    for mut upd in output.updates {
        if upd.id != node.id {
            eprintln!(
                r#"[capability] {{"iter":{},"phase":"verifier","event":"id_mismatch","got":"{}","expected":"{}"}}"#,
                iter,
                upd.id,
                node.id
            );
            upd.id = node.id.clone();
        }
        let _ = graph.update_status(&upd.id, upd.status);
        if let Some(n) = graph.get_node_mut(&upd.id) {
            n.error = upd.error;
        }
    }

    Ok(NodeOutcome { node_id: node.id.clone(), result: None, error: None, status_update: None })
}

async fn dispatch_readonly(
    node: &TaskNode,
    graph: &mut TaskGraph,
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    role_schema: &str,
    tabs: &tokio::sync::Mutex<super::llm::TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    workspace_root: &Path,
    roots: &[PathBuf],
    max_output_lines: usize,
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<NodeOutcome> {
    let input = serde_json::json!({ "node": {"id": node.id, "description": node.description} });
    let schema = "Return exactly one fenced ```json block and nothing else.\nSchema:\n{\n  \"results\": [\n    { \"id\": \"t1\", \"deltas\": [ { \"type\": \"read_file\", \"path\": \"x\" } ], \"rationale\": \"string\" }\n  ]\n}\nAllowed delta types: read_file, list_dir, read_command.\n";
    let prompt = format!(
        "{}\n\nRead-only node execution.\nWorkspace root: {}\nAction space: paths must be under workspace root.\nINPUT:\n{}",
        schema,
        workspace_root.display(),
        serde_json::to_string_pretty(&input).unwrap_or_default()
    );
    let mut payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &prompt, role_schema, "readonly", tabs, reuse_tabs, max_tabs, retries, delay_secs).await?;
    let output: ExecOutput = match parse_exec_output(&payload, &node.id) {
        Ok(v) => v,
        Err(_) => {
            let retry_prompt = format!(
                "Your response did not match the schema.\n{}\n\nInvalid response:\n{}\n\nOriginal input:\n{}",
                schema,
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
                serde_json::to_string_pretty(&input).unwrap_or_default()
            );
            let retry_payload: Value = call_agent_json_with_retry(bridge, endpoint_id, url, &retry_prompt, role_schema, "readonly", tabs, reuse_tabs, max_tabs, 1, delay_secs).await?;
            payload = retry_payload.clone();
            parse_exec_output(&retry_payload, &node.id)?
        }
    };

    let ro_path = log_dir.join(format!("iter_{:03}_readonly_output.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(ro_path, pretty);
    }
    let delta_count: usize = output.results.iter().map(|r| r.deltas.len()).sum();
    eprintln!(
        r#"[capability] {{"iter":{},"phase":"readonly","results":{},"deltas":{}}}"#,
        iter,
        output.results.len(),
        delta_count
    );

    for mut result in output.results {
        if result.id != node.id {
            eprintln!(
                r#"[capability] {{"iter":{},"phase":"readonly","event":"id_mismatch","got":"{}","expected":"{}"}}"#,
                iter,
                result.id,
                node.id
            );
            result.id = node.id.clone();
        }
        let (read_only, mutate): (Vec<Delta>, Vec<Delta>) = result
            .deltas
            .into_iter()
            .partition(|d| matches!(d, Delta::ReadFile { .. } | Delta::ListDir { .. } | Delta::ReadCommand { .. }));
        if !mutate.is_empty() {
            let msg = "read-only context received mutation deltas".to_string();
            if let Some(n) = graph.get_node_mut(&result.id) {
                n.result = Some(msg.clone());
                n.error = Some(msg.clone());
            }
            let _ = graph.update_status(&result.id, Status::Failed);
            continue;
        }
        let (out, _results, err) = apply_read_only(&read_only, roots, max_output_lines);
        let _ = graph.update_status(&result.id, Status::Running);
        if let Some(n) = graph.get_node_mut(&result.id) {
            n.result = Some(out);
            n.error = err.clone();
        }
        let has_err = err.is_some();
        if has_err {
            let summary = serde_json::json!({
                "iter": iter,
                "phase": "readonly",
                "event": "delta_error",
                "node": result.id,
                "deltas": summarize_deltas(&read_only),
            });
            let _ = std::fs::write(
                log_dir.join(format!("iter_{:03}_readonly_error.json", iter)),
                serde_json::to_string_pretty(&summary).unwrap_or_default(),
            );
            eprintln!("[capability] {}", summary);
            // Read-only failures are retryable; do not fail the node.
            let _ = graph.update_status(&result.id, Status::Ready);
        } else {
            let _ = graph.update_status(&result.id, Status::Completed);
        }
    }

    Ok(NodeOutcome { node_id: node.id.clone(), result: None, error: None, status_update: None })
}

fn parse_exec_output(payload: &Value, default_id: &str) -> Result<ExecOutput> {
    if let Ok(v) = serde_json::from_value::<ExecOutput>(payload.clone()) {
        return Ok(v);
    }
    // Fallback: accept top-level { deltas: [...] } or { phase, deltas, rationale }.
    if let Some(deltas) = payload.get("deltas") {
        let deltas: Vec<Delta> = serde_json::from_value(deltas.clone()).context("executor deltas field invalid")?;
        let rationale = payload.get("rationale").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let id = payload.get("id").and_then(|v| v.as_str()).unwrap_or(default_id).to_string();
        return Ok(ExecOutput { results: vec![ExecNodeResult { id, deltas, rationale }] });
    }
    Err(anyhow::anyhow!("executor output did not match schema"))
}

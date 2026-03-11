use super::config::CapabilityConfig;
use super::console;
use super::dag::{ContextSnapshotNode, ExecutionNode, NodeAuthority};
use super::endpoint_scheduler;
use super::engine::{self, TabManagerHandle};
use crate::ws_server::WsBridge;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
pub struct NodeDispatchContext {
    pub endpoint_id: String,
    pub url: String,
    pub max_tabs: usize,
    pub stateful: bool,
    pub role_markdown: String,
    pub workspace_root: PathBuf,
    pub log_dir: PathBuf,
}
fn resolve_role_prompt_markdown(raw: &str) -> String {
    if raw.contains('\n') || raw.contains("```") {
        return raw.to_string();
    }
    let prompt_path = PathBuf::from("/workspace/ai_sandbox/canon/canon-agent-prompts")
        .join(raw);
    if let Ok(text) = std::fs::read_to_string(&prompt_path) {
        return text;
    }
    raw.to_string()
}
pub async fn node_dispatch_resolve_endpoint(
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<std::collections::HashMap<String, usize>>,
    exec_role: &str,
    fallback: (&str, &str, usize, bool, &str),
    workspace_root: PathBuf,
    log_dir: PathBuf,
) -> NodeDispatchContext {
    let selected = endpoint_scheduler::endpoint_selector_select_endpoints_for_role(
            config,
            role_rr,
            exec_role,
            1,
        )
        .await;
    let (endpoint_id, url, max_tabs, role_markdown) = selected
        .get(0)
        .map(|e| {
            let role_markdown = config
                .endpoint_by_id(&e.id)
                .map(|cfg| cfg.role_markdown.clone())
                .unwrap_or_else(|_| fallback.4.to_string());
            (e.id.clone(), e.url.clone(), e.max_tabs, role_markdown)
        })
        .unwrap_or_else(|| (
            fallback.0.to_string(),
            fallback.1.to_string(),
            fallback.2,
            fallback.4.to_string(),
        ));
    let role_markdown = resolve_role_prompt_markdown(&role_markdown);
    NodeDispatchContext {
        endpoint_id,
        url,
        max_tabs,
        stateful: fallback.3,
        role_markdown,
        workspace_root,
        log_dir,
    }
}
pub fn log_node_dispatch(node: &ExecutionNode, mode_label: &str, endpoint_id: &str) {
    let node_type_str = format!("{:?}", node.node_type).to_lowercase();
    let caps_str = node
        .required_capabilities
        .iter()
        .map(|c| format!("{:?}", c).to_lowercase())
        .collect::<Vec<_>>()
        .join(",");
    eprintln!(
        "{}", console::console_format_info("dispatch", &
        format!("node={} type={} mode={} caps=[{}] endpoint={}", node.id, node_type_str,
        mode_label, caps_str, endpoint_id))
    );
}
pub async fn dispatch_node_call(
    node: ExecutionNode,
    auth: NodeAuthority,
    bridge: &WsBridge,
    tabs: &TabManagerHandle,
    sem: Arc<Semaphore>,
    ctx: NodeDispatchContext,
    context: Vec<ContextSnapshotNode>,
    iter: u64,
    retry_count: u32,
    retry_delay: u64,
    tab_cooldown_ms: u64,
) -> Result<(String, Result<engine::ModuleNodeCallResult>, Duration)> {
    let start = std::time::Instant::now();
    let _permit = sem.acquire().await.map_err(|_| anyhow::anyhow!("semaphore closed"))?;
    let res = engine::module_call_node(
            &node,
            &auth,
            bridge,
            &ctx.endpoint_id,
            &ctx.url,
            ctx.stateful,
            &ctx.role_markdown,
            tabs,
            ctx.max_tabs,
            tab_cooldown_ms,
            &ctx.workspace_root,
            &context,
            &ctx.log_dir,
            iter,
            retry_count,
            retry_delay,
        )
        .await;
    Ok((node.id.clone(), res, start.elapsed()))
}

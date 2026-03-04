use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Semaphore;

use super::config::CapabilityConfig;
use super::dag::{AuthorityContext, TaskNode};
use super::endpoint_scheduler;
use super::engine;
use super::tab_management::TabsHandle;
use super::console;
use crate::ws_server::WsBridge;

pub struct DispatchCtx {
    pub endpoint_id: String,
    pub url: String,
    pub max_tabs: usize,
    pub stateful: bool,
    pub workspace_root: PathBuf,
    pub log_dir: PathBuf,
}

pub async fn resolve_endpoint(
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<std::collections::HashMap<String, usize>>,
    exec_role: &str,
    fallback: (&str, &str, usize, bool),
    workspace_root: PathBuf,
    log_dir: PathBuf,
) -> DispatchCtx {
    let selected = endpoint_scheduler::select_endpoints_for_role(config, role_rr, exec_role, 1).await;
    let (endpoint_id, url, max_tabs) = selected
        .get(0)
        .map(|e| (e.id.clone(), e.url.clone(), e.max_tabs))
        .unwrap_or_else(|| (fallback.0.to_string(), fallback.1.to_string(), fallback.2));
    DispatchCtx {
        endpoint_id,
        url,
        max_tabs,
        stateful: fallback.3,
        workspace_root,
        log_dir,
    }
}

pub fn log_dispatch(node: &TaskNode, mode_label: &str, endpoint_id: &str) {
    let node_type_str = format!("{:?}", node.node_type).to_lowercase();
    let caps_str = node
        .required_capabilities
        .iter()
        .map(|c| format!("{:?}", c).to_lowercase())
        .collect::<Vec<_>>()
        .join(",");
    eprintln!(
        "{}",
        console::info(
            "dispatch",
            &format!(
                "node={} type={} mode={} caps=[{}] endpoint={}",
                node.id,
                node_type_str,
                mode_label,
                caps_str,
                endpoint_id
            )
        )
    );
}

pub async fn dispatch_node_call(
    node: TaskNode,
    auth: AuthorityContext,
    bridge: &WsBridge,
    tabs: &TabsHandle,
    sem: Arc<Semaphore>,
    ctx: DispatchCtx,
    context: Vec<engine::ContextNode>,
    iter: u64,
    retry_count: u32,
    retry_delay: u64,
    tab_cooldown_ms: u64,
) -> Result<(String, Result<engine::NodeCallResult>, Duration)> {
    let start = std::time::Instant::now();
    let _permit = sem
        .acquire()
        .await
        .map_err(|_| anyhow::anyhow!("semaphore closed"))?;
    let res = engine::call_node(
        &node,
        &auth,
        bridge,
        &ctx.endpoint_id,
        &ctx.url,
        ctx.stateful,
        "",
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

use anyhow::Result;
use std::path::Path;

use super::authority::AuthorityContext;
use super::dag::TaskGraph;
use super::engine::dispatch_node;
use crate::ws_server::WsBridge;

pub async fn verify_node(
    node: &super::dag::TaskNode,
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
    log_dir: &Path,
    iter: u64,
    retries: u32,
    delay_secs: u64,
) -> Result<()> {
    let _ = dispatch_node(
        node,
        ctx,
        graph,
        bridge,
        endpoint_id,
        url,
        role_schema,
        tabs,
        reuse_tabs,
        max_tabs,
        workspace_root,
        &[],
        0,
        log_dir,
        iter,
        retries,
        delay_secs,
    )
    .await?;
    Ok(())
}

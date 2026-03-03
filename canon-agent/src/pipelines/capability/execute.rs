use anyhow::Result;
use std::path::{Path, PathBuf};

use super::authority::AuthorityContext;
use super::capability::Capability;
use super::dag::{Status, TaskGraph, TaskNode};
use super::engine::dispatch_node;
use crate::ws_server::WsBridge;

pub async fn execute_node(
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
        roots,
        max_output_lines,
        log_dir,
        iter,
        retries,
        delay_secs,
    )
    .await?;

    // Only verify path updates Completed/Failed.
    if ctx.is_verify_context() {
        if ctx.has(Capability::StatusUpdateOnly) {
            let _ = graph.update_status(&node.id, Status::Running);
        }
    }

    Ok(())
}

use std::path::Path;

use anyhow::Result;

use super::capability_cost::CapabilityCostTable;
use super::config::CapabilityConfig;
use super::dag;
use super::graph_algo::{emit_planned_graph, run_graph_algorithms};
use super::graph_runtime::{enforce_semantic_validations, prune_unlinked_nodes};
use super::scheduler::{apply_recovery, prune_low_value_nodes};
use super::LOG_ROOT;

pub struct MaintenanceCtx<'a> {
    pub graph: &'a mut dag::TaskGraph,
    pub config: &'a CapabilityConfig,
    pub iter: u64,
    pub features_retry_rate: f64,
    pub features_failed_fraction: f64,
    pub cost_table: &'a mut CapabilityCostTable,
}

pub fn maintain_graph(ctx: MaintenanceCtx<'_>) -> Result<()> {
    super::ensure_unique_node_ids(&mut ctx.graph.nodes);
    let iter_u32 = u32::try_from(ctx.iter).unwrap_or(u32::MAX);
    emit_planned_graph(ctx.graph, Path::new(LOG_ROOT), iter_u32);
    run_graph_algorithms(ctx.graph, Path::new(LOG_ROOT), iter_u32);

    if ctx.config.prune_unlinked {
        prune_unlinked_nodes(ctx.graph);
    }
    enforce_semantic_validations(ctx.graph)?;
    prune_low_value_nodes(ctx.graph, ctx.iter, ctx.config);
    if ctx.features_retry_rate > ctx.config.recovery_retry_rate_threshold
        || ctx.features_failed_fraction > ctx.config.recovery_failed_fraction_threshold
    {
        apply_recovery(ctx.graph);
    }
    ctx.cost_table.save();
    Ok(())
}

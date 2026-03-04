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
    pub features_branching_factor: f64,
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
    let risk = risk_score(
        ctx.features_retry_rate,
        ctx.features_failed_fraction,
        ctx.features_branching_factor,
    );
    let threshold = (ctx.config.recovery_retry_rate_threshold + ctx.config.recovery_failed_fraction_threshold) / 2.0;
    if risk > threshold {
        apply_recovery(ctx.graph);
    }
    ctx.cost_table.save();
    Ok(())
}

fn risk_score(retry_rate: f64, failed_fraction: f64, branching_factor: f64) -> f64 {
    let w1 = 0.5;
    let w2 = 0.4;
    let w3 = 0.1;
    let branching_norm = (branching_factor / 4.0).min(1.0);
    (w1 * retry_rate) + (w2 * failed_fraction) + (w3 * branching_norm)
}

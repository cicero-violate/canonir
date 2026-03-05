use std::path::Path;

use anyhow::Result;

use super::dag;
use super::graph_algo::{emit_planned_graph, node_utility, run_graph_algorithms};
use super::graph_runtime::{enforce_semantic_validations, prune_unlinked_nodes};

pub struct MaintenanceCtx<'a> {
    pub graph: &'a mut dag::TaskGraph,
    pub log_dir: &'a Path,
    pub iter: u64,
    pub features_retry_rate: f64,
    pub features_failed_fraction: f64,
    pub features_branching_factor: f64,
    pub prune_unlinked: bool,
    pub auto_prune: bool,
    pub prune_min_age: u64,
    pub prune_threshold: f64,
    pub recovery_retry_rate_threshold: f64,
    pub recovery_failed_fraction_threshold: f64,
}

pub(crate) fn prune_low_value_nodes(graph: &mut dag::TaskGraph, iter: u64, auto_prune: bool, prune_min_age: u64, prune_threshold: f64) {
    if !auto_prune {
        return;
    }
    let mut parents = std::collections::HashSet::new();
    for node in &graph.nodes {
        for dep in &node.deps {
            parents.insert(dep.clone());
        }
    }
    let mut pruned = Vec::new();
    for node in &graph.nodes {
        if node.status != dag::Status::Completed {
            continue;
        }
        if node.deps.is_empty() {
            continue;
        }
        if parents.contains(&node.id) {
            continue;
        }
        let age = node.completed_iter.map(|t| iter.saturating_sub(t)).unwrap_or(0);
        if age < prune_min_age {
            continue;
        }
        let util = node_utility(graph, &node.id, iter);
        if util < prune_threshold {
            pruned.push(node.id.clone());
        }
    }
    if pruned.is_empty() {
        return;
    }
    graph.nodes.retain(|n| !pruned.contains(&n.id));
    for node in &mut graph.nodes {
        node.deps.retain(|d| !pruned.contains(d));
    }
    graph.rebuild_index();
}

pub(crate) fn apply_recovery(graph: &mut dag::TaskGraph) {
    for node in &mut graph.nodes {
        if node.status == dag::Status::Failed {
            node.status = dag::Status::Pending;
            node.readonly_fail_count = 0;
            node.error = None;
            node.result = None;
        }
    }
    graph.rebuild_index();
}

pub fn maintain_graph(ctx: MaintenanceCtx<'_>) -> Result<()> {
    super::ensure_unique_node_ids(&mut ctx.graph.nodes);
    let iter_u32 = u32::try_from(ctx.iter).unwrap_or(u32::MAX);
    emit_planned_graph(ctx.graph, ctx.log_dir, iter_u32);
    run_graph_algorithms(ctx.graph, ctx.log_dir, iter_u32);

    if ctx.prune_unlinked {
        prune_unlinked_nodes(ctx.graph);
    }
    enforce_semantic_validations(ctx.graph)?;
    prune_low_value_nodes(ctx.graph, ctx.iter, ctx.auto_prune, ctx.prune_min_age, ctx.prune_threshold);
    let risk = risk_score(ctx.features_retry_rate, ctx.features_failed_fraction, ctx.features_branching_factor);
    let threshold = (ctx.recovery_retry_rate_threshold + ctx.recovery_failed_fraction_threshold) / 2.0;
    if risk > threshold {
        apply_recovery(ctx.graph);
    }
    Ok(())
}

fn risk_score(retry_rate: f64, failed_fraction: f64, branching_factor: f64) -> f64 {
    let w1 = 0.5;
    let w2 = 0.4;
    let w3 = 0.1;
    let branching_norm = (branching_factor / 4.0).min(1.0);
    (w1 * retry_rate) + (w2 * failed_fraction) + (w3 * branching_norm)
}

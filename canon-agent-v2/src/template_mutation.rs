use super::capability::PipelineCapability;
use super::dag::ExecutionGraph;
use super::graph_algo::{compute_graph_features, graph_analysis_compute_graph_signals, score_node_utility, GraphAnalysis, GraphFeatureVector};
use std::collections::{HashMap, HashSet};
pub struct GraphMutationCandidateScore {
    pub graph: ExecutionGraph,
    pub features: GraphFeatureVector,
    pub score: f64,
}
pub fn generate_mutation_candidates(
    graph: &ExecutionGraph,
    count: usize,
    mutation_budget: usize,
    mutation_rate: f64,
    iter: u64,
    targets: &[String],
) -> Vec<ExecutionGraph> {
    let mut out = Vec::new();
    if count == 0 {
        return out;
    }
    let features = compute_graph_features(graph);
    let target_nodes = mutation_target_filter(graph, targets);
    let target_scope = expand_targets_to_radius(graph, &target_nodes);
    for i in 0..count {
        let mode = select_mutation_mode(&features, iter, i as u64);
        let candidate = mutate_graph_with_mode(
            graph,
            mode,
            mutation_budget,
            mutation_rate,
            iter,
            &target_scope,
        );
        out.push(candidate);
    }
    out
}
pub fn score_mutation_candidates(
    candidates: Vec<ExecutionGraph>,
    iter: u64,
) -> Vec<GraphMutationCandidateScore> {
    use rayon::prelude::*;
    candidates
        .into_par_iter()
        .map(|g| {
            let features = compute_graph_features(&g);
            let node_utility_avg = compute_node_utility_avg(&g, iter);
            let score = compute_mutation_score(&features, node_utility_avg);
            GraphMutationCandidateScore { graph: g, features, score }
        })
        .collect()
}
fn compute_mutation_score(features: &GraphFeatureVector, node_utility_avg: f64) -> f64 {
    let w1 = 1.0;
    let w2 = 0.7;
    let w3 = 0.5;
    let lambda_c = 0.8;
    let lambda_d = 0.6;
    let mu = 0.4;
    let base = (w1 * features.completion_velocity)
        - (w2 * features.failed_fraction)
        - (w3 * features.blocked_fraction);
    base - (lambda_c * features.cycle_frequency) - (lambda_d * features.deadlock_rate) + (mu * node_utility_avg)
}
fn mutate_graph_with_mode(
    graph: &ExecutionGraph,
    mode: usize,
    mutation_budget: usize,
    mutation_rate: f64,
    iter: u64,
    target_scope: &[String],
) -> ExecutionGraph {
    let base_signals = graph_analysis_compute_graph_signals(graph);
    let base_features = compute_graph_features(graph);
    let mut g = graph.clone();
    if mutation_rate <= 0.0 || mutation_budget == 0 {
        return g;
    }
    let target_set: HashSet<String> = target_scope.iter().cloned().collect();
    let mut remaining = mutation_budget;
    match mode {
        0 => {
            if remaining > 0 {
                remaining -= mutate_node_descriptions(&mut g, &target_set);
            }
        }
        1 => {
            if remaining > 0 {
                remaining -= mutate_node_capabilities(&mut g, &target_set);
            }
        }
        2 => {
            if remaining > 0 {
                remaining -= drop_low_utility_nodes(&mut g, iter, &target_set);
            }
        }
        _ => {
            if remaining > 0 {
                remaining -= mutate_edges(&mut g, &target_set);
            }
        }
    }
    if mutation_degrades_graph(&base_signals, &base_features, &g) {
        return graph.clone();
    }
    g
}
fn mutate_node_descriptions(graph: &mut ExecutionGraph, target_set: &HashSet<String>) -> usize {
    let mut changed = 0;
    for n in &mut graph.nodes {
        if !target_set.is_empty() && !target_set.contains(&n.id) {
            continue;
        }
        let lower = n.description.to_lowercase();
        if lower.contains("cargo build") {
            n.description = n.description.replace("cargo build", "cargo check");
            changed += 1;
            break;
        }
    }
    changed
}
fn mutate_node_capabilities(graph: &mut ExecutionGraph, target_set: &HashSet<String>) -> usize {
    for n in &mut graph.nodes {
        if !target_set.is_empty() && !target_set.contains(&n.id) {
            continue;
        }
        if n.required_capabilities.contains(&PipelineCapability::CargoBuild) {
            n.required_capabilities = n.required_capabilities.iter().map(|c| if *c == PipelineCapability::CargoBuild { PipelineCapability::CargoCheck } else { *c }).collect();
            return 1;
        }
    }
    0
}
fn drop_low_utility_nodes(
    graph: &mut ExecutionGraph,
    iter: u64,
    target_set: &HashSet<String>,
) -> usize {
    let mut worst: Option<(usize, f64)> = None;
    for (idx, n) in graph.nodes.iter().enumerate() {
        if !target_set.is_empty() && !target_set.contains(&n.id) {
            continue;
        }
        let util = score_node_utility(graph, &n.id, iter);
        if util < 0.0 {
            if worst.map(|w| util < w.1).unwrap_or(true) {
                worst = Some((idx, util));
            }
        }
    }
    if let Some((idx, _)) = worst {
        let id = graph.nodes[idx].id.clone();
        graph.nodes.remove(idx);
        for node in &mut graph.nodes {
            node.deps.retain(|d| d != &id);
        }
        graph.rebuild_index();
        return 1;
    }
    0
}
fn mutate_edges(graph: &mut ExecutionGraph, target_set: &HashSet<String>) -> usize {
    for node in &mut graph.nodes {
        if !target_set.is_empty() && !target_set.contains(&node.id) {
            continue;
        }
        if node.deps.len() > 1 {
            node.deps.pop();
            return 1;
        }
    }
    0
}

fn mutation_degrades_graph(
    base: &GraphAnalysis,
    base_features: &GraphFeatureVector,
    candidate: &ExecutionGraph,
) -> bool {
    let next = graph_analysis_compute_graph_signals(candidate);
    let next_features = compute_graph_features(candidate);
    let base_cycles = count_cycles(base);
    let next_cycles = count_cycles(&next);
    if next_cycles > base_cycles {
        return true;
    }
    if next.unreachable.len() > base.unreachable.len() {
        return true;
    }
    if candidate.nodes.is_empty() {
        return true;
    }
    if next_features.deadlock_rate > base_features.deadlock_rate + 0.05 {
        return true;
    }
    if next_features.blocked_fraction > base_features.blocked_fraction + 0.05 {
        return true;
    }
    if next_features.failed_fraction > base_features.failed_fraction + 0.05 {
        return true;
    }
    false
}

fn count_cycles(signals: &GraphAnalysis) -> usize {
    signals.sccs.iter().filter(|comp| comp.len() > 1).count()
}

pub fn mutation_target_filter(graph: &ExecutionGraph, targets: &[String]) -> Vec<String> {
    if targets.is_empty() {
        return Vec::new();
    }
    let targets_lower: Vec<String> = targets.iter().map(|t| t.to_lowercase()).collect();
    let mut matched = Vec::new();
    for n in &graph.nodes {
        let hay = format!("{} {}", n.id, n.description).to_lowercase();
        if targets_lower.iter().any(|t| hay.contains(t)) {
            matched.push(n.id.clone());
        }
    }
    matched
}

fn expand_targets_to_radius(graph: &ExecutionGraph, targets: &[String]) -> Vec<String> {
    if targets.is_empty() {
        return Vec::new();
    }
    let radius = dynamic_target_radius(targets.len());
    if radius == 0 {
        return targets.to_vec();
    }
    let mut neighbors: HashMap<String, Vec<String>> = HashMap::new();
    for n in &graph.nodes {
        neighbors.entry(n.id.clone()).or_default();
        for dep in &n.deps {
            neighbors.entry(n.id.clone()).or_default().push(dep.clone());
            neighbors.entry(dep.clone()).or_default().push(n.id.clone());
        }
    }
    let mut visited: HashSet<String> = HashSet::new();
    let mut frontier: Vec<String> = targets.to_vec();
    for t in &frontier {
        visited.insert(t.clone());
    }
    for _ in 0..radius {
        let mut next = Vec::new();
        for node in &frontier {
            if let Some(adj) = neighbors.get(node) {
                for neighbor in adj {
                    if visited.insert(neighbor.clone()) {
                        next.push(neighbor.clone());
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    visited.into_iter().collect()
}

fn dynamic_target_radius(targets_len: usize) -> usize {
    if targets_len == 0 {
        return 0;
    }
    let raw = (targets_len as f64).sqrt().floor() as usize + 1;
    raw.min(3)
}

fn select_mutation_mode(features: &GraphFeatureVector, iter: u64, idx: u64) -> usize {
    let mut weights = [0.5_f64; 4];
    // 0: description mutation
    weights[0] += features.failure_rate * 2.0;
    weights[0] += features.blocked_fraction * 1.5;
    // 1: capability mutation
    weights[1] += (1.0 - features.completion_velocity).max(0.0) * 1.2;
    // 2: drop/prune nodes
    weights[2] += features.deadlock_rate * 2.5;
    weights[2] += features.failed_fraction * 1.5;
    // 3: edge mutation
    weights[3] += features.cycle_frequency * 2.5;
    weights[3] += features.blocked_fraction * 1.0;
    let sum: f64 = weights.iter().sum();
    if sum <= 0.0 {
        return ((iter + idx) % 4) as usize;
    }
    // periodic exploration to avoid local minima
    if iter % 7 == 0 {
        return ((iter + idx) % 4) as usize;
    }
    let seed = (iter.wrapping_mul(1103515245)).wrapping_add(idx.wrapping_mul(12345));
    let pick = (seed % 10_000) as f64 / 10_000.0 * sum;
    let mut acc = 0.0;
    for (i, w) in weights.iter().enumerate() {
        acc += *w;
        if pick <= acc {
            return i;
        }
    }
    0
}

fn compute_node_utility_avg(graph: &ExecutionGraph, iter: u64) -> f64 {
    if graph.nodes.is_empty() {
        return 0.0;
    }
    let mut total = 0.0;
    for node in &graph.nodes {
        total += score_node_utility(graph, &node.id, iter);
    }
    total / graph.nodes.len() as f64
}

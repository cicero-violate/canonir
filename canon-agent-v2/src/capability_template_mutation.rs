use super::capability::PipelineCapability;
use super::dag::ExecutionGraph;
use super::graph_algo::{compute_graph_features, score_node_utility, GraphFeatureVector};
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
) -> Vec<ExecutionGraph> {
    let mut out = Vec::new();
    if count == 0 {
        return out;
    }
    for i in 0..count {
        let mode = ((iter as usize) + i) % 4;
        let candidate = mutate_graph_with_mode(
            graph,
            mode,
            mutation_budget,
            mutation_rate,
            iter,
        );
        out.push(candidate);
    }
    out
}
pub fn score_mutation_candidates(
    candidates: Vec<ExecutionGraph>,
) -> Vec<GraphMutationCandidateScore> {
    candidates
        .into_iter()
        .map(|g| {
            let features = compute_graph_features(&g);
            let score = compute_mutation_score(&features);
            GraphMutationCandidateScore {
                graph: g,
                features,
                score,
            }
        })
        .collect()
}
fn compute_mutation_score(features: &GraphFeatureVector) -> f64 {
    let w1 = 1.0;
    let w2 = 0.7;
    let w3 = 0.5;
    (w1 * features.completion_velocity) - (w2 * features.failed_fraction)
        - (w3 * features.blocked_fraction)
}
fn mutate_graph_with_mode(
    graph: &ExecutionGraph,
    mode: usize,
    mutation_budget: usize,
    mutation_rate: f64,
    iter: u64,
) -> ExecutionGraph {
    let mut g = graph.clone();
    if mutation_rate <= 0.0 || mutation_budget == 0 {
        return g;
    }
    let mut remaining = mutation_budget;
    match mode {
        0 => {
            if remaining > 0 {
                remaining -= mutate_node_descriptions(&mut g);
            }
        }
        1 => {
            if remaining > 0 {
                remaining -= mutate_node_capabilities(&mut g);
            }
        }
        2 => {
            if remaining > 0 {
                remaining -= drop_low_utility_nodes(&mut g, iter);
            }
        }
        _ => {
            if remaining > 0 {
                remaining -= mutate_edges(&mut g);
            }
        }
    }
    g
}
fn mutate_node_descriptions(graph: &mut ExecutionGraph) -> usize {
    let mut changed = 0;
    for n in &mut graph.nodes {
        let lower = n.description.to_lowercase();
        if lower.contains("cargo build") {
            n.description = n.description.replace("cargo build", "cargo check");
            changed += 1;
            break;
        }
    }
    changed
}
fn mutate_node_capabilities(graph: &mut ExecutionGraph) -> usize {
    for n in &mut graph.nodes {
        if n.required_capabilities.contains(&PipelineCapability::CargoBuild) {
            n.required_capabilities = n
                .required_capabilities
                .iter()
                .map(|c| {
                    if *c == PipelineCapability::CargoBuild {
                        PipelineCapability::CargoCheck
                    } else {
                        *c
                    }
                })
                .collect();
            return 1;
        }
    }
    0
}
fn drop_low_utility_nodes(graph: &mut ExecutionGraph, iter: u64) -> usize {
    let mut worst: Option<(usize, f64)> = None;
    for (idx, n) in graph.nodes.iter().enumerate() {
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
fn mutate_edges(graph: &mut ExecutionGraph) -> usize {
    for node in &mut graph.nodes {
        if node.deps.len() > 1 {
            node.deps.pop();
            return 1;
        }
    }
    0
}

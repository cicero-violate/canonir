use super::capability::Capability;
use super::dag::TaskGraph;
use super::graph_algo::node_utility;

pub fn generate_candidates(
    graph: &TaskGraph,
    count: usize,
    mutation_budget: usize,
    mutation_rate: f64,
    iter: u64,
) -> Vec<TaskGraph> {
    let mut out = Vec::new();
    if count == 0 {
        return out;
    }
    for i in 0..count {
        let mode = ((iter as usize) + i) % 4;
        let candidate = mutate_template_with_mode(graph, mode, mutation_budget, mutation_rate, iter);
        out.push(candidate);
    }
    out
}

fn mutate_template_with_mode(
    graph: &TaskGraph,
    mode: usize,
    mutation_budget: usize,
    mutation_rate: f64,
    iter: u64,
) -> TaskGraph {
    let mut g = graph.clone();
    if mutation_rate <= 0.0 || mutation_budget == 0 {
        return g;
    }
    let mut remaining = mutation_budget;
    match mode {
        0 => {
            if remaining > 0 {
                remaining -= rewrite_descriptions(&mut g);
            }
        }
        1 => {
            if remaining > 0 {
                remaining -= mutate_capabilities(&mut g);
            }
        }
        2 => {
            if remaining > 0 {
                remaining -= drop_low_utility(&mut g, iter);
            }
        }
        _ => {
            if remaining > 0 {
                remaining -= edge_mutation(&mut g);
            }
        }
    }
    g
}

fn rewrite_descriptions(graph: &mut TaskGraph) -> usize {
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

fn mutate_capabilities(graph: &mut TaskGraph) -> usize {
    for n in &mut graph.nodes {
        if n.required_capabilities.contains(&Capability::CargoBuild) {
            n.required_capabilities = n.required_capabilities.iter().map(|c| {
                if *c == Capability::CargoBuild { Capability::CargoCheck } else { *c }
            }).collect();
            return 1;
        }
    }
    0
}

fn drop_low_utility(graph: &mut TaskGraph, iter: u64) -> usize {
    let mut worst: Option<(usize, f64)> = None;
    for (idx, n) in graph.nodes.iter().enumerate() {
        let util = node_utility(graph, &n.id, iter);
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

fn edge_mutation(graph: &mut TaskGraph) -> usize {
    for node in &mut graph.nodes {
        if node.deps.len() > 1 {
            node.deps.pop();
            return 1;
        }
    }
    0
}

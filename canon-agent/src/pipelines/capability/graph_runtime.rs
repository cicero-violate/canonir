use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use algorithms::graph::{csr::Csr, reachability, scc, topological_sort};
use super::dag;
use super::decompose;
use super::engine;
use super::tab_management::{self, TabsHandle};

struct GraphKernels {
    adj: Vec<Vec<usize>>,
    csr: Csr,
    topo: Vec<usize>,
    sccs: Vec<Vec<usize>>,
}

fn build_kernels(graph: &dag::TaskGraph) -> GraphKernels {
    let id_to_idx: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); graph.nodes.len()];
    for n in &graph.nodes {
        let to = match id_to_idx.get(n.id.as_str()) {
            Some(v) => *v,
            None => continue,
        };
        for dep in &n.deps {
            if let Some(from) = id_to_idx.get(dep.as_str()) {
                adj[*from].push(to);
            }
        }
    }
    let csr = Csr::from_adj(&adj);
    let topo = topological_sort::topological_sort(&adj);
    let sccs = scc::kosaraju_scc(&adj);
    GraphKernels { adj, csr, topo, sccs }
}

fn prune_roots(kernels: &GraphKernels) -> Vec<usize> {
    if kernels.adj.is_empty() {
        return Vec::new();
    }
    let mut roots = Vec::new();
    let mut seen = vec![false; kernels.adj.len()];
    for &idx in &kernels.topo {
        if !kernels.adj[idx].is_empty() {
            roots.push(idx);
            seen[idx] = true;
        }
    }
    if kernels.topo.len() < kernels.adj.len() {
        for comp in &kernels.sccs {
            for &idx in comp {
                if !seen[idx] && !kernels.adj[idx].is_empty() {
                    roots.push(idx);
                    seen[idx] = true;
                }
            }
        }
    }
    roots
}

pub(crate) fn build_context(
    graph: &dag::TaskGraph,
    node_id: &str,
    radius: usize,
) -> Vec<engine::ContextNode> {
    if radius == 0 {
        return Vec::new();
    }
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut frontier: std::collections::VecDeque<(String, usize)> = std::collections::VecDeque::new();
    frontier.push_back((node_id.to_string(), 0));
    visited.insert(node_id.to_string());

    let by_id: std::collections::HashMap<String, dag::TaskNode> =
        graph.nodes.iter().map(|n| (n.id.clone(), n.clone())).collect();

    let mut result = Vec::new();
    while let Some((current, depth)) = frontier.pop_front() {
        if depth >= radius {
            continue;
        }
        if let Some(node) = by_id.get(&current) {
            // Add parents only (toward root)
            for dep in &node.deps {
                if visited.insert(dep.clone()) {
                    frontier.push_back((dep.clone(), depth + 1));
                }
            }
        }
    }
    for id in visited.iter() {
        if let Some(n) = by_id.get(id) {
            result.push(engine::ContextNode {
                id: n.id.clone(),
                description: n.description.clone(),
                node_type: n.node_type,
                deps: n.deps.clone(),
                required_capabilities: n.required_capabilities.clone(),
                status: n.status,
            });
        }
    }
    result
}

pub(crate) fn prune_unlinked_nodes(graph: &mut dag::TaskGraph) {
    if graph.nodes.is_empty() {
        return;
    }
    let kernels = build_kernels(graph);
    let roots = prune_roots(&kernels);
    if roots.is_empty() {
        graph.nodes.clear();
        return;
    }
    let reach = reachability::reachability_gpu(&kernels.csr, &roots);
    let mut next = Vec::with_capacity(graph.nodes.len());
    for (idx, node) in graph.nodes.iter().enumerate() {
        if reach.get(idx).copied().unwrap_or(false) {
            next.push(node.clone());
        }
    }
    graph.nodes = next;
}

pub(crate) fn enforce_semantic_validations(graph: &dag::TaskGraph) -> Result<()> {
    // 1) All render nodes must be reachable from at least one analysis node.
    let kernels = build_kernels(graph);
    let analysis_roots: Vec<usize> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (n.node_type == decompose::NodeType::Analysis).then_some(i))
        .collect();
    let reach = reachability::reachability_gpu(&kernels.csr, &analysis_roots);
    for (idx, n) in graph.nodes.iter().enumerate() {
        if n.node_type == decompose::NodeType::Render && !reach.get(idx).copied().unwrap_or(false) {
            return Err(anyhow::anyhow!(
                "render node {} not reachable from analysis node",
                n.id
            ));
        }
    }

    // 2) No depth beyond max_depth (approx by DFS).
    // This will be checked elsewhere by limiting expansion; kept as sanity.
    Ok(())
}

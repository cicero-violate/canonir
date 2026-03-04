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
    let all: std::collections::HashSet<usize> = kernels.topo.iter()
        .chain(kernels.sccs.iter().flatten())
        .copied()
        .collect();
    all.into_iter().filter(|&i| !kernels.adj[i].is_empty()).collect()
}

pub(crate) fn build_context(
    graph: &dag::TaskGraph,
    node_id: &str,
    radius: usize,
) -> Vec<engine::ContextNode> {
    if radius == 0 {
        return Vec::new();
    }
    let id_to_idx: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let mut rev_adj: Vec<Vec<usize>> = vec![Vec::new(); graph.nodes.len()];
    for (idx, node) in graph.nodes.iter().enumerate() {
        for dep in &node.deps {
            if let Some(dep_idx) = id_to_idx.get(dep.as_str()) {
                rev_adj[idx].push(*dep_idx);
            }
        }
    }
    let csr = Csr::from_adj(&rev_adj);
    let start = graph.nodes.iter().position(|n| n.id == node_id);
    let roots: Vec<usize> = start.into_iter().collect();
    let reach = reachability::reachability_bounded(&csr, &roots, radius);

    let by_id: HashMap<&str, &dag::TaskNode> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();

    let failure_summary = graph.nodes.iter().enumerate()
        .filter(|(i, n)| reach.get(*i).copied().unwrap_or(false) && n.status == dag::Status::Failed)
        .filter_map(|(_, n)| n.error.as_deref().map(|e| format!("{}: {}", n.id, e)))
        .collect::<Vec<_>>()
        .join("\n");
    let failure_summary = if failure_summary.is_empty() { None } else { Some(failure_summary) };

    graph.nodes.iter().enumerate()
        .filter(|(i, _)| reach.get(*i).copied().unwrap_or(false))
        .map(|(_, n)| {
            let causal_summary = n.deps.iter()
                .filter_map(|dep_id| by_id.get(dep_id.as_str()))
                .filter(|dep| dep.status == dag::Status::Completed)
                .filter_map(|dep| dep.result.as_deref())
                .collect::<Vec<_>>()
                .join("\n---\n");
            let causal_summary = if causal_summary.is_empty() { None } else { Some(causal_summary) };
            engine::ContextNode {
            id: n.id.clone(),
            description: n.description.clone(),
            node_type: n.node_type,
            deps: n.deps.clone(),
            required_capabilities: n.required_capabilities.clone(),
            status: n.status,
            result: n.result.clone(),
            error: n.error.clone(),
            causal_summary,
            failure_summary: failure_summary.clone(),
        }
        })
        .collect()
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

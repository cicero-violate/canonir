use super::dag;
use super::dag::ContextSnapshotNode;
use super::decompose;
use super::goal::GoalSpec;
#[cfg(feature = "cuda")]
use algorithms::graph::model_checking;
use algorithms::graph::{csr::Csr, reachability, scc, topological_sort};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
struct GraphRuntimeGraphKernels {
    adj: Vec<Vec<usize>>,
    csr: Csr,
    topo: Vec<usize>,
    sccs: Vec<Vec<usize>>,
}
fn graph_runtime_build_kernels(graph: &dag::ExecutionGraph) -> GraphRuntimeGraphKernels {
    let id_to_idx: HashMap<&str, usize> = graph.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
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
    GraphRuntimeGraphKernels { adj, csr, topo, sccs }
}
fn graph_runtime_prune_roots(kernels: &GraphRuntimeGraphKernels) -> Vec<usize> {
    let all: std::collections::HashSet<usize> = kernels.topo.iter().chain(kernels.sccs.iter().flatten()).copied().collect();
    all.into_iter().filter(|&i| !kernels.adj[i].is_empty()).collect()
}
pub(crate) fn collect_execution_context(graph: &dag::ExecutionGraph, node_id: &str, radius: usize) -> Vec<ContextSnapshotNode> {
    if radius == 0 {
        return Vec::new();
    }
    let id_to_idx: HashMap<&str, usize> = graph.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
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
    let by_id: HashMap<&str, &dag::ExecutionNode> = graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let failure_summary = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, n)| reach.get(*i).copied().unwrap_or(false) && n.status == dag::NodeStatus::Failed)
        .filter_map(|(_, n)| n.error.as_deref().map(|e| format!("{}: {}", n.id, e)))
        .collect::<Vec<_>>()
        .join("\n");
    let failure_summary = if failure_summary.is_empty() { None } else { Some(failure_summary) };
    let mut seen_causal: std::collections::HashSet<String> = std::collections::HashSet::new();
    graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| reach.get(*i).copied().unwrap_or(false))
        .map(|(_, n)| {
            let causal_summary = n
                .deps
                .iter()
                .filter_map(|dep_id| by_id.get(dep_id.as_str()))
                .filter(|dep| dep.status == dag::NodeStatus::Completed)
                .filter_map(|dep| {
                    if seen_causal.insert(dep.id.clone()) {
                        dep.result.as_deref()
                    } else {
                        None
                    }
                })
                .map(|r| {
                    let is_raw_io = r.starts_with("[read_file") || r.starts_with("[read_command") || r.starts_with("list_dir");
                    if is_raw_io {
                        r.lines().next().unwrap_or(r)
                    } else if r.len() > 600 {
                        &r[..600]
                    } else {
                        r
                    }
                })
                .collect::<Vec<_>>()
                .join("\n---\n");
            let causal_summary = if causal_summary.is_empty() { None } else { Some(causal_summary) };
            ContextSnapshotNode {
                id: n.id.clone(),
                description: n.description.clone(),
                node_type: n.node_type,
                deps: n.deps.clone(),
                required_capabilities: n.required_capabilities.clone(),
                status: n.status,
                result: n.result.as_deref().map(|r| {
                    let is_raw_io = r.starts_with("[read_file") || r.starts_with("[read_command") || r.starts_with("list_dir");
                    let cap = if is_raw_io { 400 } else { 800 };
                    if r.len() > cap { format!("{}…", &r[..cap]) } else { r.to_string() }
                }),
                error: n.error.clone(),
                causal_summary,
                failure_summary: failure_summary.as_deref().map(|s| if s.len() > 400 { format!("{}…", &s[..400]) } else { s.to_string() }),
            }
        })
        .collect()
}
pub(crate) fn prune_unreachable_nodes(graph: &mut dag::ExecutionGraph) {
    if graph.nodes.is_empty() {
        return;
    }
    let kernels = graph_runtime_build_kernels(graph);
    let roots = graph_runtime_prune_roots(&kernels);
    if roots.is_empty() {
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
pub(crate) fn validate_graph_semantics(graph: &dag::ExecutionGraph, goal: Option<&GoalSpec>) -> Result<()> {
    validate_graph_invariants(graph)?;
    if let Some(goal) = goal {
        validate_goal(graph, goal)?;
    }
    let kernels = graph_runtime_build_kernels(graph);
    let analysis_roots: Vec<usize> = graph.nodes.iter().enumerate().filter_map(|(i, n)| (n.node_type == decompose::DecomposeNodeType::Analysis).then_some(i)).collect();
    let reach = reachability::reachability_gpu(&kernels.csr, &analysis_roots);
    for (idx, n) in graph.nodes.iter().enumerate() {
        if n.node_type == decompose::DecomposeNodeType::Render && !reach.get(idx).copied().unwrap_or(false) {
            return Err(anyhow::anyhow!("render node {} not reachable from analysis node", n.id));
        }
    }
    let invariant_mask: Vec<u8> = graph
        .nodes
        .iter()
        .map(|n| {
            let ok = !n.description.trim().is_empty() && !n.required_capabilities.is_empty();
            ok as u8
        })
        .collect();
    #[cfg(feature = "cuda")]
    {
        let ok = model_checking::model_check_gpu(&kernels.csr, &analysis_roots, &invariant_mask);
        if !ok {
            return Err(anyhow::anyhow!("model check failed: invariant violation on reachable node"));
        }
    }
    #[cfg(not(feature = "cuda"))]
    {
        for (idx, ok) in invariant_mask.iter().enumerate() {
            if reach.get(idx).copied().unwrap_or(false) && *ok == 0 {
                return Err(anyhow::anyhow!("model check failed: invariant violation on reachable node"));
            }
        }
    }
    Ok(())
}

fn validate_goal(graph: &dag::ExecutionGraph, goal: &GoalSpec) -> Result<()> {
    if !graph.all_completed() {
        // Goal validation is enforced only at terminal state.
        return Ok(());
    }
    if goal.success_criteria.iter().any(|c| c == "graph_completed") {
        if !graph.all_completed() {
            return Err(anyhow::anyhow!("goal_not_satisfied: graph not completed"));
        }
    }
    if goal.success_criteria.iter().any(|c| c == "no_failed_nodes") {
        if graph.has_failed() {
            return Err(anyhow::anyhow!("goal_not_satisfied: failed nodes present"));
        }
    }
    if goal.success_criteria.iter().any(|c| c == "invariants_hold") {
        validate_graph_invariants(graph)?;
    }
    Ok(())
}

pub(crate) fn goal_reached(graph: &dag::ExecutionGraph, goal: &GoalSpec) -> bool {
    if !graph.all_completed() {
        return false;
    }
    if goal.success_criteria.iter().any(|c| c == "no_failed_nodes") && graph.has_failed() {
        return false;
    }
    if goal.success_criteria.iter().any(|c| c == "invariants_hold") {
        if validate_graph_invariants(graph).is_err() {
            return false;
        }
    }
    true
}
fn validate_graph_invariants(graph: &dag::ExecutionGraph) -> Result<()> {
    if graph.nodes.is_empty() {
        return Ok(());
    }
    let kernels = graph_runtime_build_kernels(graph);
    let invariant_mask: Vec<u8> = graph
        .nodes
        .iter()
        .map(|n| {
            let ok = !n.id.trim().is_empty() && !n.description.trim().is_empty() && !n.required_capabilities.is_empty() && !n.deps.iter().any(|d| d == &n.id);
            ok as u8
        })
        .collect();
    let roots: Vec<usize> = (0..graph.nodes.len()).collect();
    #[cfg(feature = "cuda")]
    {
        let ok = model_checking::model_check_gpu(&kernels.csr, &roots, &invariant_mask);
        if !ok {
            return Err(anyhow::anyhow!("assertion check failed: invariant violated"));
        }
    }
    #[cfg(not(feature = "cuda"))]
    {
        for (idx, ok) in invariant_mask.iter().enumerate() {
            if *ok == 0 {
                return Err(anyhow::anyhow!("assertion check failed: invariant violated at node {}", graph.nodes[idx].id));
            }
        }
    }
    Ok(())
}

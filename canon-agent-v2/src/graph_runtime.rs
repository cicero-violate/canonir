use super::dag;
use super::dag::ContextSnapshotNode;
use super::decompose;
use super::goal::GoalSpec;
use super::objectives;
use super::graph_algo;
#[cfg(feature = "cuda")]
use algorithms::graph::model_checking;
use algorithms::graph::{csr::Csr, reachability, scc, topological_sort};
use anyhow::Result;
use std::collections::HashMap;
struct GraphRuntimeGraphKernels {
    adj: Vec<Vec<usize>>,
    csr: Csr,
    topo: Vec<usize>,
    sccs: Vec<Vec<usize>>,
}
#[derive(Clone)]
struct GraphRuntimeKernelCache {
    adj: Vec<Vec<usize>>,
    topo: Vec<usize>,
    sccs: Vec<Vec<usize>>,
}
static GRAPH_KERNEL_CACHE: std::sync::OnceLock<
    std::sync::Mutex<(String, GraphRuntimeKernelCache)>,
> = std::sync::OnceLock::new();
fn graph_runtime_build_kernels(graph: &dag::ExecutionGraph) -> GraphRuntimeGraphKernels {
    let sig = graph_algo::hash_graph_structure(graph);
    let cache = GRAPH_KERNEL_CACHE
        .get_or_init(|| std::sync::Mutex::new((
            String::new(),
            GraphRuntimeKernelCache {
                adj: Vec::new(),
                topo: Vec::new(),
                sccs: Vec::new(),
            },
        )));
    if let Ok(guard) = cache.lock() {
        if guard.0 == sig && !guard.1.adj.is_empty() {
            let adj = guard.1.adj.clone();
            let csr = Csr::from_adj(&adj);
            let topo = guard.1.topo.clone();
            let sccs = guard.1.sccs.clone();
            return GraphRuntimeGraphKernels {
                adj,
                csr,
                topo,
                sccs,
            };
        }
    }
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
    let kernels = GraphRuntimeGraphKernels {
        adj,
        csr,
        topo,
        sccs,
    };
    if let Ok(mut guard) = cache.lock() {
        *guard = (
            sig,
            GraphRuntimeKernelCache {
                adj: kernels.adj.clone(),
                topo: kernels.topo.clone(),
                sccs: kernels.sccs.clone(),
            },
        );
    }
    kernels
}
fn graph_runtime_prune_roots(kernels: &GraphRuntimeGraphKernels) -> Vec<usize> {
    let all: std::collections::HashSet<usize> = kernels
        .topo
        .iter()
        .chain(kernels.sccs.iter().flatten())
        .copied()
        .collect();
    all.into_iter().filter(|&i| !kernels.adj[i].is_empty()).collect()
}
#[allow(dead_code)]
pub(crate) fn collect_execution_context(
    graph: &dag::ExecutionGraph,
    node_id: &str,
    radius: usize,
) -> Vec<ContextSnapshotNode> {
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
    let by_id: HashMap<&str, &dag::ExecutionNode> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();
    let failure_summary = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, n)| {
            reach.get(*i).copied().unwrap_or(false)
                && n.status == dag::NodeStatus::Failed
        })
        .filter_map(|(_, n)| n.error.as_deref().map(|e| format!("{}: {}", n.id, e)))
        .collect::<Vec<_>>()
        .join("\n");
    let failure_summary = if failure_summary.is_empty() {
        None
    } else {
        Some(failure_summary)
    };
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
                    let is_raw_io = r.starts_with("[read_file")
                        || r.starts_with("[read_command") || r.starts_with("list_dir");
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
            let causal_summary = if causal_summary.is_empty() {
                None
            } else {
                Some(causal_summary)
            };
            ContextSnapshotNode {
                id: n.id.clone(),
                description: n.description.clone(),
                node_type: n.node_type,
                deps: n.deps.clone(),
                required_capabilities: n.required_capabilities.clone(),
                status: n.status,
                result: n
                    .result
                    .as_deref()
                    .map(|r| {
                        let is_raw_io = r.starts_with("[read_file")
                            || r.starts_with("[read_command")
                            || r.starts_with("list_dir");
                        let cap = if is_raw_io { 400 } else { 800 };
                        if r.len() > cap {
                            format!("{}…", & r[..cap])
                        } else {
                            r.to_string()
                        }
                    }),
                error: n.error.clone(),
                causal_summary,
                failure_summary: failure_summary
                    .as_deref()
                    .map(|s| {
                        if s.len() > 400 {
                            format!("{}…", & s[..400])
                        } else {
                            s.to_string()
                        }
                    }),
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
pub(crate) fn must_validate_graph_semantics(
    graph: &dag::ExecutionGraph,
    goal: Option<&GoalSpec>,
) -> Result<()> {
    must_validate_graph_invariants(graph)?;
    if let Some(goal) = goal {
        validate_goal(graph, goal)?;
    }
    let kernels = graph_runtime_build_kernels(graph);
    let analysis_roots: Vec<usize> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            (n.node_type == decompose::DecomposeNodeType::Analysis).then_some(i)
        })
        .collect();
    let reach = reachability::reachability_gpu(&kernels.csr, &analysis_roots);
    for (idx, n) in graph.nodes.iter().enumerate() {
        if n.node_type == decompose::DecomposeNodeType::Render
            && !reach.get(idx).copied().unwrap_or(false)
        {
            return Err(
                anyhow::anyhow!("render node {} not reachable from analysis node", n.id),
            );
        }
    }
    let invariant_mask: Vec<u8> = graph
        .nodes
        .iter()
        .map(|n| {
            let ok = !n.description.trim().is_empty()
                && !n.required_capabilities.is_empty();
            ok as u8
        })
        .collect();
    #[cfg(feature = "cuda")]
    {
        let ok = model_checking::model_check_gpu(
            &kernels.csr,
            &analysis_roots,
            &invariant_mask,
        );
        if !ok {
            return Err(
                anyhow::anyhow!(
                    "model check failed: invariant violation on reachable node"
                ),
            );
        }
    }
    #[cfg(not(feature = "cuda"))]
    {
        for (idx, ok) in invariant_mask.iter().enumerate() {
            if reach.get(idx).copied().unwrap_or(false) && *ok == 0 {
                return Err(
                    anyhow::anyhow!(
                        "model check failed: invariant violation on reachable node"
                    ),
                );
            }
        }
    }
    Ok(())
}
#[allow(dead_code)]
pub(crate) fn ensure_render_reachable(graph: &mut dag::ExecutionGraph) -> bool {
    let analysis_ids: Vec<String> = graph
        .nodes
        .iter()
        .filter(|n| n.node_type == decompose::DecomposeNodeType::Analysis)
        .map(|n| n.id.clone())
        .collect();
    if analysis_ids.is_empty() {
        return false;
    }
    let kernels = graph_runtime_build_kernels(graph);
    let analysis_roots: Vec<usize> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            (n.node_type == decompose::DecomposeNodeType::Analysis).then_some(i)
        })
        .collect();
    let reach = reachability::reachability_gpu(&kernels.csr, &analysis_roots);
    let mut changed = false;
    for (idx, n) in graph.nodes.iter_mut().enumerate() {
        if n.node_type != decompose::DecomposeNodeType::Render {
            continue;
        }
        let ok = reach.get(idx).copied().unwrap_or(false);
        if ok {
            continue;
        }
        let dep = analysis_ids[0].clone();
        if !n.deps.contains(&dep) {
            n.deps.push(dep);
            changed = true;
        }
    }
    if changed {
        graph.rebuild_index();
    }
    changed
}
fn validate_goal(graph: &dag::ExecutionGraph, goal: &GoalSpec) -> Result<()> {
    if !graph.all_completed() {
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
        must_validate_graph_invariants(graph)?;
    }
    if goal.success_criteria.iter().any(|c| c == "objective_improved") {
        if objectives::objective_reward_delta() <= 0.0 {
            return Err(anyhow::anyhow!("goal_not_satisfied: objective did not improve"));
        }
    }
    Ok(())
}
#[allow(dead_code)]
pub(crate) fn goal_reached(graph: &dag::ExecutionGraph, goal: &GoalSpec) -> bool {
    if !graph.all_completed() {
        return false;
    }
    if goal.success_criteria.iter().any(|c| c == "no_failed_nodes") && graph.has_failed()
    {
        return false;
    }
    if goal.success_criteria.iter().any(|c| c == "invariants_hold") {
        if must_validate_graph_invariants(graph).is_err() {
            return false;
        }
    }
    if goal.success_criteria.iter().any(|c| c == "objective_improved") {
        if objectives::objective_reward_delta() <= 0.0 {
            return false;
        }
    }
    true
}
fn must_validate_graph_invariants(graph: &dag::ExecutionGraph) -> Result<()> {
    if graph.nodes.is_empty() {
        return Ok(());
    }
    let mut ids = std::collections::HashSet::new();
    for n in &graph.nodes {
        if !ids.insert(n.id.as_str()) {
            return Err(
                anyhow::anyhow!("assertion check failed: duplicate node id {}", n.id),
            );
        }
    }
    let id_set: std::collections::HashSet<&str> = graph
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .collect();
    for n in &graph.nodes {
        let mut dep_set = std::collections::HashSet::new();
        for dep in &n.deps {
            if dep == &n.id {
                return Err(
                    anyhow::anyhow!("assertion check failed: self-dependency {}", n.id),
                );
            }
            if !id_set.contains(dep.as_str()) {
                return Err(
                    anyhow::anyhow!(
                        "assertion check failed: missing dependency {} for node {}", dep,
                        n.id
                    ),
                );
            }
            if !dep_set.insert(dep.as_str()) {
                return Err(
                    anyhow::anyhow!(
                        "assertion check failed: duplicate dependency {} on node {}",
                        dep, n.id
                    ),
                );
            }
        }
    }
    let kernels = graph_runtime_build_kernels(graph);
    let invariant_mask: Vec<u8> = graph
        .nodes
        .iter()
        .map(|n| {
            let ok = !n.id.trim().is_empty() && !n.description.trim().is_empty()
                && !n.required_capabilities.is_empty()
                && !n.deps.iter().any(|d| d == &n.id);
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
                return Err(
                    anyhow::anyhow!(
                        "assertion check failed: invariant violated at node {}", graph
                        .nodes[idx].id
                    ),
                );
            }
        }
    }
    Ok(())
}

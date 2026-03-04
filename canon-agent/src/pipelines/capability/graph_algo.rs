use std::collections::{HashMap, VecDeque};
use std::path::Path;

use algorithms::graph::adj_list::AdjList;

use super::{dag, decompose};

pub fn emit_planned_graph(graph: &dag::TaskGraph, log_dir: &Path, iter: u32) {
    #[derive(serde::Serialize)]
    struct GraphSnapshot<'a> {
        nodes: Vec<GraphNode<'a>>,
        edges: Vec<GraphEdge<'a>>,
    }
    #[derive(serde::Serialize)]
    struct GraphNode<'a> {
        id: &'a str,
        deps: &'a [String],
        node_type: decompose::NodeType,
    }
    #[derive(serde::Serialize)]
    struct GraphEdge<'a> {
        from: &'a str,
        to: &'a str,
    }

    let mut edges = Vec::new();
    for n in &graph.nodes {
        for dep in &n.deps {
            edges.push(GraphEdge { from: dep.as_str(), to: n.id.as_str() });
        }
    }
    let nodes = graph
        .nodes
        .iter()
        .map(|n| GraphNode {
            id: n.id.as_str(),
            deps: &n.deps,
            node_type: n.node_type,
        })
        .collect();
    let snapshot = GraphSnapshot { nodes, edges };
    let path = if iter == 0 {
        log_dir.join("planned_graph.json")
    } else {
        log_dir.join(format!("iter_{:03}_planned_graph.json", iter))
    };
    if let Ok(pretty) = serde_json::to_string_pretty(&snapshot) {
        let _ = std::fs::write(path, pretty);
    }
}

pub fn run_graph_algorithms(graph: &dag::TaskGraph, log_dir: &Path, iter: u32) {
    let signals = compute_graph_signals(graph);
    let mut id_to_index: HashMap<String, usize> = HashMap::new();
    let mut index_to_id: Vec<String> = Vec::new();
    for n in &graph.nodes {
        let idx = index_to_id.len();
        id_to_index.insert(n.id.clone(), idx);
        index_to_id.push(n.id.clone());
    }
    let mut adj = AdjList::new(graph.nodes.len());
    for n in &graph.nodes {
        let to = match id_to_index.get(&n.id) {
            Some(v) => *v,
            None => continue,
        };
        for dep in &n.deps {
            if let Some(from) = id_to_index.get(dep) {
                adj.add_edge(*from, to);
            }
        }
    }

    #[cfg(feature = "cuda")]
    {
        let csr = adj.to_csr();
        let levels = algorithms::graph::gpu::bfs_gpu(&csr, 0);
        let snapshot = serde_json::json!({
            "algorithm": "bfs_gpu",
            "source": 0,
            "levels": levels,
            "index_to_id": index_to_id,
            "signals": signals.to_json(&index_to_id),
        });
        let path = if iter == 0 {
            log_dir.join("graph_algorithms.json")
        } else {
            log_dir.join(format!("iter_{:03}_graph_algorithms.json", iter))
        };
        if let Ok(pretty) = serde_json::to_string_pretty(&snapshot) {
            let _ = std::fs::write(path, pretty);
        }
    }

    #[cfg(not(feature = "cuda"))]
    {
        let snapshot = serde_json::json!({
            "algorithm": "bfs_gpu",
            "status": "skipped",
            "reason": "canon-agent built without feature \"cuda\"",
            "signals": signals.to_json(&index_to_id),
        });
        let path = if iter == 0 {
            log_dir.join("graph_algorithms.json")
        } else {
            log_dir.join(format!("iter_{:03}_graph_algorithms.json", iter))
        };
        if let Ok(pretty) = serde_json::to_string_pretty(&snapshot) {
            let _ = std::fs::write(path, pretty);
        }
    }
}

#[derive(Debug, Clone)]
struct GraphSignals {
    roots: Vec<usize>,
    topo_order: Vec<usize>,
    sccs: Vec<Vec<usize>>,
    unreachable: Vec<usize>,
    has_cycle: bool,
}

impl GraphSignals {
    fn to_json(&self, index_to_id: &[String]) -> serde_json::Value {
        let to_ids = |idxs: &[usize]| idxs.iter().filter_map(|i| index_to_id.get(*i)).cloned().collect::<Vec<_>>();
        let scc_ids = self.sccs.iter().map(|comp| to_ids(comp)).collect::<Vec<_>>();
        serde_json::json!({
            "roots": to_ids(&self.roots),
            "topo_order": to_ids(&self.topo_order),
            "sccs": scc_ids,
            "unreachable": to_ids(&self.unreachable),
            "has_cycle": self.has_cycle
        })
    }
}

fn compute_graph_signals(graph: &dag::TaskGraph) -> GraphSignals {
    let n = graph.nodes.len();
    let mut id_to_index: HashMap<&str, usize> = HashMap::new();
    for (idx, node) in graph.nodes.iter().enumerate() {
        id_to_index.insert(node.id.as_str(), idx);
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indegree = vec![0usize; n];
    for node in &graph.nodes {
        let to = match id_to_index.get(node.id.as_str()) {
            Some(v) => *v,
            None => continue,
        };
        for dep in &node.deps {
            if let Some(from) = id_to_index.get(dep.as_str()) {
                adj[*from].push(to);
                indegree[to] += 1;
            }
        }
    }
    let roots: Vec<usize> = indegree
        .iter()
        .enumerate()
        .filter_map(|(i, &d)| if d == 0 { Some(i) } else { None })
        .collect();
    let topo_order = algorithms::graph::topological_sort::topological_sort(&adj);
    let has_cycle = topo_order.len() != n;
    let sccs = algorithms::graph::scc::kosaraju_scc(&adj)
        .into_iter()
        .filter(|c| c.len() > 1)
        .collect::<Vec<_>>();

    #[cfg(feature = "cuda")]
    let reach = {
        let csr = algorithms::graph::csr::Csr::from_adj(&adj);
        reachability_mask(&csr, &roots)
    };
    #[cfg(not(feature = "cuda"))]
    let reach = reachability_mask(&adj, &roots);

    let unreachable = reach
        .iter()
        .enumerate()
        .filter_map(|(i, &ok)| if ok { None } else { Some(i) })
        .collect::<Vec<_>>();
    GraphSignals {
        roots,
        topo_order,
        sccs,
        unreachable,
        has_cycle,
    }
}

#[cfg(feature = "cuda")]
fn reachability_mask(adj_csr: &algorithms::graph::csr::Csr, roots: &[usize]) -> Vec<bool> {
    algorithms::graph::reachability::reachability_gpu(adj_csr, roots)
}

#[cfg(not(feature = "cuda"))]
fn reachability_mask(adj: &[Vec<usize>], roots: &[usize]) -> Vec<bool> {
    let n = adj.len();
    let mut visited = vec![false; n];
    let mut q = VecDeque::new();
    for &r in roots {
        if r < n && !visited[r] {
            visited[r] = true;
            q.push_back(r);
        }
    }
    while let Some(u) = q.pop_front() {
        for &v in &adj[u] {
            if v < n && !visited[v] {
                visited[v] = true;
                q.push_back(v);
            }
        }
    }
    visited
}

pub fn planner_signals_for_graph(graph: &dag::TaskGraph) -> String {
    let signals = compute_graph_signals(graph);
    let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let to_id = |i: usize| ids.get(i).copied().unwrap_or("<unknown>");
    let roots = signals.roots.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(", ");
    let unreachable = signals.unreachable.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(", ");
    let topo = signals.topo_order.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(", ");
    let sccs = signals
        .sccs
        .iter()
        .map(|comp| comp.iter().map(|&i| to_id(i)).collect::<Vec<_>>().join(" -> "))
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "roots=[{}]; unreachable=[{}]; topo_order=[{}]; sccs=[{}]; has_cycle={}",
        roots, unreachable, topo, sccs, signals.has_cycle
    )
}

pub fn enforce_linking_constraints(graph: &dag::TaskGraph) -> Result<(), String> {
    // Linker may intentionally leave nodes unconnected. Only ensure no self-cycle at this layer.
    for n in &graph.nodes {
        if n.deps.iter().any(|d| d == &n.id) {
            return Err(format!("self-cycle for node {}", n.id));
        }
    }
    Ok(())
}

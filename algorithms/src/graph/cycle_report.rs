use std::collections::HashSet;

use super::scc::kosaraju_scc;

/// Topological sort that also reports cycles (as SCCs).
pub fn topological_sort_with_cycles(adj: &[Vec<usize>]) -> (Vec<usize>, Vec<Vec<usize>>) {
    let n = adj.len();
    if n == 0 {
        return (Vec::new(), Vec::new());
    }

    let mut indegree = vec![0usize; n];
    for edges in adj {
        for &v in edges {
            if v < n {
                indegree[v] += 1;
            }
        }
    }

    let mut order = Vec::with_capacity(n);
    let mut queue: std::collections::VecDeque<usize> = indegree.iter().enumerate().filter_map(|(i, &deg)| if deg == 0 { Some(i) } else { None }).collect();
    while let Some(u) = queue.pop_front() {
        order.push(u);
        for &v in &adj[u] {
            if v >= n {
                continue;
            }
            indegree[v] = indegree[v].saturating_sub(1);
            if indegree[v] == 0 {
                queue.push_back(v);
            }
        }
    }

    if order.len() == n {
        return (order, Vec::new());
    }

    let mut cycles = Vec::new();
    let mut self_loops = HashSet::new();
    for (u, edges) in adj.iter().enumerate() {
        if edges.iter().any(|&v| v == u) {
            self_loops.insert(u);
        }
    }
    for scc in kosaraju_scc(adj) {
        if scc.len() > 1 || scc.iter().any(|v| self_loops.contains(v)) {
            cycles.push(scc);
        }
    }

    (order, cycles)
}

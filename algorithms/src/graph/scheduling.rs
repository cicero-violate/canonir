use std::collections::VecDeque;

/// Topological scheduling: returns layers of nodes that can execute in parallel.
///
/// `adj[u]` is the list of outgoing edges from u to v.
pub fn topological_layers(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut indegree = vec![0usize; adj.len()];
    for edges in adj {
        for &v in edges {
            if v < indegree.len() {
                indegree[v] += 1;
            }
        }
    }

    let mut queue = VecDeque::new();
    for i in 0..adj.len() {
        if indegree[i] == 0 {
            queue.push_back(i);
        }
    }

    let mut layers = Vec::new();
    while !queue.is_empty() {
        let level_count = queue.len();
        let mut layer = Vec::with_capacity(level_count);
        for _ in 0..level_count {
            if let Some(u) = queue.pop_front() {
                layer.push(u);
                for &v in &adj[u] {
                    if v < indegree.len() {
                        indegree[v] -= 1;
                        if indegree[v] == 0 {
                            queue.push_back(v);
                        }
                    }
                }
            }
        }
        if !layer.is_empty() {
            layers.push(layer);
        }
    }
    layers
}

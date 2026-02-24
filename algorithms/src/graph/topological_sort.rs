use std::collections::VecDeque;

pub fn topological_sort(adj: &[Vec<usize>]) -> Vec<usize> {
    let mut indegree = vec![0; adj.len()];
    for edges in adj {
        for &v in edges {
            indegree[v] += 1;
        }
    }

    let mut queue = VecDeque::new();
    for i in 0..adj.len() {
        if indegree[i] == 0 {
            queue.push_back(i);
        }
    }

    let mut order = Vec::new();
    while let Some(u) = queue.pop_front() {
        order.push(u);
        for &v in &adj[u] {
            indegree[v] -= 1;
            if indegree[v] == 0 {
                queue.push_back(v);
            }
        }
    }
    order
}

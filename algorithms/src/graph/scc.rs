//! Strongly connected components (Kosaraju).
//!
//! Returns SCCs as vectors of node indices.

pub fn kosaraju_scc(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    if n == 0 {
        return Vec::new();
    }

    // First pass: compute finish order on original graph.
    let mut visited = vec![false; n];
    let mut order: Vec<usize> = Vec::with_capacity(n);

    fn dfs_finish(v: usize, adj: &[Vec<usize>], visited: &mut [bool], order: &mut Vec<usize>) {
        visited[v] = true;
        for &u in &adj[v] {
            if !visited[u] {
                dfs_finish(u, adj, visited, order);
            }
        }
        order.push(v);
    }

    for v in 0..n {
        if !visited[v] {
            dfs_finish(v, adj, &mut visited, &mut order);
        }
    }

    // Reverse graph.
    let mut rev: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (v, edges) in adj.iter().enumerate() {
        for &u in edges {
            rev[u].push(v);
        }
    }

    // Second pass: DFS on reversed graph in decreasing finish order.
    visited.fill(false);
    let mut comps: Vec<Vec<usize>> = Vec::new();

    fn dfs_collect(v: usize, adj: &[Vec<usize>], visited: &mut [bool], comp: &mut Vec<usize>) {
        visited[v] = true;
        comp.push(v);
        for &u in &adj[v] {
            if !visited[u] {
                dfs_collect(u, adj, visited, comp);
            }
        }
    }

    for &v in order.iter().rev() {
        if !visited[v] {
            let mut comp = Vec::new();
            dfs_collect(v, &rev, &mut visited, &mut comp);
            comps.push(comp);
        }
    }

    comps
}

pub fn dfs(adj: &[Vec<usize>], start: usize) -> Vec<usize> {
    fn visit(node: usize, adj: &[Vec<usize>], visited: &mut [bool], out: &mut Vec<usize>) {
        visited[node] = true;
        out.push(node);
        for &n in &adj[node] {
            if !visited[n] {
                visit(n, adj, visited, out);
            }
        }
    }

    let mut visited = vec![false; adj.len()];
    let mut order = Vec::new();
    visit(start, adj, &mut visited, &mut order);
    order
}

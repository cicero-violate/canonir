//! Reachability — forward BFS/DFS from a root set.
//!
//! Variables:
//!   adj    : &[Vec<usize>]   — adjacency list, vertex count = adj.len()
//!   roots  : &[usize]        — starting vertices
//!   reach  : Vec<bool>       — reach[v] = true iff v is reachable from roots
//!
//! Equation:
//!   reach = BFS({ roots }) on adj
//!   live(v) <=> reach[v]

/// Returns a boolean mask: `mask[v]` is true iff `v` is reachable from any root.
pub fn reachability(adj: &[Vec<usize>], roots: &[usize]) -> Vec<bool> {
    let n = adj.len();
    let mut visited = vec![false; n];
    let mut queue = std::collections::VecDeque::new();
    for &r in roots {
        if r < n && !visited[r] {
            visited[r] = true;
            queue.push_back(r);
        }
    }
    while let Some(u) = queue.pop_front() {
        for &w in &adj[u] {
            if w < n && !visited[w] {
                visited[w] = true;
                queue.push_back(w);
            }
        }
    }
    visited
}

/// Returns true iff the graph has no cycle (i.e. is a DAG).
/// Uses DFS-colouring: white=0, grey=1, black=2.
pub fn is_acyclic(adj: &[Vec<usize>]) -> bool {
    let n = adj.len();
    let mut colour = vec![0u8; n];

    fn visit(u: usize, adj: &[Vec<usize>], colour: &mut Vec<u8>) -> bool {
        colour[u] = 1; // grey — on stack
        for &w in &adj[u] {
            if w >= adj.len() { continue; }
            if colour[w] == 1 { return false; } // back edge = cycle
            if colour[w] == 0 && !visit(w, adj, colour) { return false; }
        }
        colour[u] = 2; // black — done
        true
    }

    for u in 0..n {
        if colour[u] == 0 && !visit(u, adj, &mut colour) {
            return false;
        }
    }
    true
}

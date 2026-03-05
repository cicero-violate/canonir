//! CFG structural pattern matching.
//!
//! Variables:
//!   blocks : &[BasicBlock]  — flat CFG block list
//!   adj    : &[Vec<usize>]  — CFG successor adjacency
//!
//! Equations:
//!   iterator_loop(b) = true iff b contains a SwitchInt on Option discriminant
//!     with one arm mapping to a loop-body block and one to a loop-exit block
//!
//!   for_loop_pattern:
//!     loop_head   -> call .next() -> store result
//!     switch_head -> SwitchInt(discriminant) -> [exit_arm, body_arm]
//!     body_arm    -> loop body blocks -> goto loop_head
//!     exit_arm    -> post-loop continuation
//!
//! Used by: canon-capture analyze_switch_structure to recognize iterator patterns.

use std::collections::HashSet;

/// Describes a detected iterator for-loop in the CFG.
#[derive(Debug, Clone)]
pub struct IteratorLoopPattern {
    /// Block that calls `.next()` and holds the Option result.
    pub loop_head: usize,
    /// Block containing the SwitchInt on the Option discriminant.
    pub switch_block: usize,
    /// Block index of the Some arm (loop body entry).
    pub body_entry: usize,
    /// Block index of the None arm (loop exit).
    pub exit_block: usize,
    /// All blocks belonging to the loop body (reachable from body_entry, not through exit).
    pub body_blocks: HashSet<usize>,
}

/// Detect iterator for-loop patterns in a CFG.
///
/// A switch block is an iterator loop switch iff:
///   - it has exactly 2 successors
///   - one successor (exit) has no back-edge to the switch block's predecessor chain
///   - the other successor (body) eventually reaches a block that jumps back to loop_head
///
/// Returns one entry per detected loop.
pub fn detect_iterator_loops(adj: &[Vec<usize>], back_edges: &HashSet<(usize, usize)>) -> Vec<IteratorLoopPattern> {
    let n = adj.len();
    let mut patterns = Vec::new();

    for switch_block in 0..n {
        let succs = &adj[switch_block];
        if succs.len() != 2 {
            continue;
        }
        let (s0, s1) = (succs[0], succs[1]);

        // Determine which successor is the loop body vs exit by back-edge presence.
        // The body successor should have a back-edge somewhere in its reachable set.
        let s0_has_back = reachable_has_back_edge(s0, adj, back_edges, switch_block);
        let s1_has_back = reachable_has_back_edge(s1, adj, back_edges, switch_block);

        let (body_entry, exit_block) = match (s0_has_back, s1_has_back) {
            (true, false) => (s0, s1),
            (false, true) => (s1, s0),
            _ => continue, // ambiguous or not a loop
        };

        // Find loop_head: the predecessor of switch_block that is the .next() call site.
        // In practice this is the immediate dominator of switch_block within the loop.
        let loop_head = find_loop_head(switch_block, adj, back_edges);

        // Collect all body blocks: reachable from body_entry without crossing exit_block.
        let body_blocks = collect_body_blocks(body_entry, adj, exit_block, switch_block);

        patterns.push(IteratorLoopPattern { loop_head, switch_block, body_entry, exit_block, body_blocks });
    }
    patterns
}

/// Compute back edges in the CFG using DFS.
/// A back edge (u, v) means v is an ancestor of u in the DFS tree.
pub fn compute_back_edges(adj: &[Vec<usize>]) -> HashSet<(usize, usize)> {
    let n = adj.len();
    let mut visited = vec![false; n];
    let mut on_stack = vec![false; n];
    let mut back_edges = HashSet::new();
    for start in 0..n {
        if !visited[start] {
            dfs_back_edges(start, adj, &mut visited, &mut on_stack, &mut back_edges);
        }
    }
    back_edges
}

fn dfs_back_edges(u: usize, adj: &[Vec<usize>], visited: &mut Vec<bool>, on_stack: &mut Vec<bool>, back_edges: &mut HashSet<(usize, usize)>) {
    visited[u] = true;
    on_stack[u] = true;
    for &v in &adj[u] {
        if v >= adj.len() {
            continue;
        }
        if !visited[v] {
            dfs_back_edges(v, adj, visited, on_stack, back_edges);
        } else if on_stack[v] {
            back_edges.insert((u, v));
        }
    }
    on_stack[u] = false;
}

fn reachable_has_back_edge(start: usize, adj: &[Vec<usize>], back_edges: &HashSet<(usize, usize)>, stop_at: usize) -> bool {
    let n = adj.len();
    let mut visited = vec![false; n];
    let mut stack = vec![start];
    while let Some(u) = stack.pop() {
        if u == stop_at || visited[u] {
            continue;
        }
        visited[u] = true;
        for &v in &adj[u] {
            if v < n {
                if back_edges.contains(&(u, v)) {
                    return true;
                }
                stack.push(v);
            }
        }
    }
    false
}

fn find_loop_head(switch_block: usize, adj: &[Vec<usize>], back_edges: &HashSet<(usize, usize)>) -> usize {
    // Loop head is the target of any back-edge that reaches switch_block.
    for &(src, dst) in back_edges {
        if dst <= switch_block && src >= switch_block {
            return dst;
        }
    }
    switch_block
}

fn collect_body_blocks(entry: usize, adj: &[Vec<usize>], exit_block: usize, switch_block: usize) -> HashSet<usize> {
    let n = adj.len();
    let mut visited = HashSet::new();
    let mut stack = vec![entry];
    while let Some(u) = stack.pop() {
        if u == exit_block || u == switch_block || visited.contains(&u) {
            continue;
        }
        visited.insert(u);
        for &v in &adj[u] {
            if v < n {
                stack.push(v);
            }
        }
    }
    visited
}

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A* search on a weighted directed graph.
///
/// `graph[u]` = Vec of (v, weight).
/// `heuristic(v)` must be admissible for optimality.
///
/// Returns (cost, path) from start to goal if reachable.
pub fn a_star(
    graph: &[Vec<(usize, u64)>],
    start: usize,
    goal: usize,
    heuristic: impl Fn(usize) -> u64,
) -> Option<(u64, Vec<usize>)> {
    if start >= graph.len() || goal >= graph.len() {
        return None;
    }

    let n = graph.len();
    let mut g_score = vec![u64::MAX; n];
    let mut came_from: Vec<Option<usize>> = vec![None; n];
    let mut open = BinaryHeap::new();

    g_score[start] = 0;
    open.push((Reverse(heuristic(start)), start));

    while let Some((Reverse(_f), u)) = open.pop() {
        if u == goal {
            return Some((g_score[u], reconstruct_path(goal, &came_from)));
        }
        let base = g_score[u];
        if base == u64::MAX {
            continue;
        }
        for &(v, w) in &graph[u] {
            let cand = base.saturating_add(w);
            if cand < g_score[v] {
                g_score[v] = cand;
                came_from[v] = Some(u);
                let f = cand.saturating_add(heuristic(v));
                open.push((Reverse(f), v));
            }
        }
    }
    None
}

fn reconstruct_path(goal: usize, came_from: &[Option<usize>]) -> Vec<usize> {
    let mut path = Vec::new();
    let mut cur = Some(goal);
    while let Some(v) = cur {
        path.push(v);
        cur = came_from[v];
    }
    path.reverse();
    path
}

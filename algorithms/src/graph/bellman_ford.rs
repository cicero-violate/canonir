//! Bellman-Ford single-source shortest paths.
//!
//! Variables:
//!   V       = number of vertices
//!   E       = edge list as (u, v, weight)
//!   dist[v] = shortest known distance from source to v
//!   INF     = u64::MAX / 2  (sentinel for unreachable)
//!
//! Equations:
//!   Initialise: dist[source] = 0,  dist[v] = INF  for v != source
//!
//!   Relax (repeated V-1 times):
//!     for each (u, v, w) in E:
//!       dist[v] = min(dist[v], dist[u] + w)
//!
//!   Negative cycle detection (pass V):
//!     if any edge still relaxes => negative cycle exists
//!
//!   Complexity: O(V * E)

pub const INF: u64 = u64::MAX / 2;

/// Returns None if a negative-weight cycle is reachable from source.
/// Otherwise returns dist[] where dist[v] = shortest path length source->v.
pub fn bellman_ford(v: usize, edges: &[(usize, usize, u64)], source: usize) -> Option<Vec<u64>> {
    let mut dist = vec![INF; v];
    dist[source] = 0;

    if v == 0 {
        return Some(dist);
    }

    for _ in 0..v - 1 {
        let mut updated = false;

        for &(u, w, weight) in edges {
            if dist[u] != INF {
                let candidate = dist[u].saturating_add(weight);
                if candidate < dist[w] {
                    dist[w] = candidate;
                    updated = true;
                }
            }
        }

        if !updated {
            break;
        }
    }

    // V-th pass: negative cycle check
    for &(u, w, weight) in edges {
        if dist[u] != INF {
            let candidate = dist[u].saturating_add(weight);
            if candidate < dist[w] {
                return None;
            }
        }
    }

    Some(dist)
}

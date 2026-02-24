//! FFI bridge to graph/bellman_ford.cu via unified libgpu.a.
//!
//! Variables:
//!   edges_flat : flat (u, v, w) triples as u64, length E*3
//!   V, E       : vertex and edge counts
//!   source     : start vertex
//!
//! Equation:
//!   dist[v] = shortest path source->v, or INF if unreachable
//!   Returns Err if a negative-weight cycle is reachable from source

use super::bellman_ford::INF;

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_bellman_ford(edges_flat: *const u64, v: i32, e: i32, source: i32, dist_out: *mut u64) -> i32;
}

/// Run GPU Bellman-Ford. Returns Ok(dist) or Err("negative cycle").
#[cfg(feature = "cuda")]
pub fn bellman_ford_gpu(v: usize, edges: &[(usize, usize, u64)], source: usize) -> Result<Vec<u64>, &'static str> {
    // flatten edges to (u, v, w) u64 triples
    let flat: Vec<u64> = edges.iter().flat_map(|&(u, v, weight)| [u as u64, v as u64, weight]).collect();

    let mut dist = vec![INF; v];

    let neg_cycle = unsafe { gpu_bellman_ford(flat.as_ptr(), v as i32, edges.len() as i32, source as i32, dist.as_mut_ptr()) };

    if neg_cycle != 0 {
        Err("negative cycle detected")
    } else {
        Ok(dist)
    }
}

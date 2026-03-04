//! Reachability via GPU BFS (no CPU implementation).
//!
//! Variables:
//!   adj (CSR): row_ptr, col_idx
//!   roots: starting vertices
//!   visited[v] = true iff v reachable from any root

use super::csr::Csr;
use super::gpu;

#[cfg(not(feature = "cuda"))]
compile_error!("graph::reachability requires feature \"cuda\" (GPU-only module)");

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_reachability(
        row_ptr: *const i32,
        col_idx: *const i32,
        v: i32,
        e: i32,
        roots: *const i32,
        root_count: i32,
        visited_out: *mut i32,
    );
}

/// GPU reachability from a set of roots. Returns visited mask.
#[cfg(feature = "cuda")]
pub fn reachability_gpu(csr: &Csr, roots: &[usize]) -> Vec<bool> {
    let mut visited = vec![0i32; csr.vertex_count()];
    let roots_i32: Vec<i32> = roots.iter().map(|&r| r as i32).collect();
    unsafe {
        gpu_reachability(
            csr.row_ptr.as_ptr(),
            csr.col_idx.as_ptr(),
            csr.vertex_count() as i32,
            csr.edge_count() as i32,
            roots_i32.as_ptr(),
            roots_i32.len() as i32,
            visited.as_mut_ptr(),
        );
    }
    visited.into_iter().map(|v| v != 0).collect()
}

/// GPU reachability from roots bounded by max_depth (inclusive).
#[cfg(feature = "cuda")]
pub fn reachability_bounded(csr: &Csr, roots: &[usize], max_depth: usize) -> Vec<bool> {
    if csr.vertex_count() == 0 {
        return Vec::new();
    }
    if max_depth == 0 {
        return vec![false; csr.vertex_count()];
    }
    let mut visited = vec![false; csr.vertex_count()];
    for &root in roots {
        let levels = gpu::bfs_gpu(csr, root);
        for (idx, lvl) in levels.into_iter().enumerate() {
            if lvl >= 0 && (lvl as usize) <= max_depth {
                visited[idx] = true;
            }
        }
    }
    visited
}

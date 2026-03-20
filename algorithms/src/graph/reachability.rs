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
    fn gpu_reachability(row_ptr: *const i32, col_idx: *const i32, v: i32, e: i32, roots: *const i32, root_count: i32, visited_out: *mut i32);
    fn gpu_reachability_batched(row_ptr: *const i32, col_idx: *const i32, v: i32, e: i32, roots: *const i32, r: i32, out: *mut i32);
    fn gpu_reachability_tc(row_ptr: *const i32, col_idx: *const i32, v: i32, e: i32, w: i32, max_iters: i32, out: *mut u64);
}

/// GPU reachability from a set of roots. Returns visited mask.
#[cfg(feature = "cuda")]
pub fn reachability_gpu(csr: &Csr, roots: &[usize]) -> Vec<bool> {
    let mut visited = vec![0i32; csr.vertex_count()];
    let roots_i32: Vec<i32> = roots.iter().map(|&r| r as i32).collect();
    unsafe {
        gpu_reachability(csr.row_ptr.as_ptr(), csr.col_idx.as_ptr(), csr.vertex_count() as i32, csr.edge_count() as i32, roots_i32.as_ptr(), roots_i32.len() as i32, visited.as_mut_ptr());
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

/// GPU batched reachability — one BFS per root in parallel.
/// Returns flat R×V row-major matrix: out[r*V + v] = true if v reachable from roots[r].
#[cfg(feature = "cuda")]
pub fn reachability_batched_gpu(csr: &Csr, roots: &[usize]) -> Vec<Vec<bool>> {
    let r = roots.len();
    let v = csr.vertex_count();
    if r == 0 || v == 0 {
        return vec![vec![false; v]; r];
    }
    let out = reachability_batched_flat_gpu(csr, roots);
    out.chunks(v).map(|row| row.iter().map(|&x| x != 0).collect()).collect()
}

/// GPU batched reachability — returns flat R×V row-major i32 matrix.
#[cfg(feature = "cuda")]
pub fn reachability_batched_flat_gpu(csr: &Csr, roots: &[usize]) -> Vec<i32> {
    let r = roots.len();
    let v = csr.vertex_count();
    if r == 0 || v == 0 {
        return Vec::new();
    }
    let roots_i32: Vec<i32> = roots.iter().map(|&x| x as i32).collect();
    let mut out = vec![0i32; r * v];
    unsafe {
        gpu_reachability_batched(csr.row_ptr.as_ptr(), csr.col_idx.as_ptr(), v as i32, csr.edge_count() as i32, roots_i32.as_ptr(), r as i32, out.as_mut_ptr());
    }
    out
}

/// GPU transitive closure using bitset propagation.
/// Returns flat V×W row-major bitset matrix where W = ceil(V/64).
#[cfg(feature = "cuda")]
pub fn reachability_tc_gpu(csr: &Csr, max_iters: usize) -> Vec<u64> {
    let v = csr.vertex_count();
    if v == 0 {
        return Vec::new();
    }
    let w = (v + 63) / 64;
    let mut out = vec![0u64; v * w];
    unsafe {
        gpu_reachability_tc(csr.row_ptr.as_ptr(), csr.col_idx.as_ptr(), v as i32, csr.edge_count() as i32, w as i32, max_iters as i32, out.as_mut_ptr());
    }
    out
}

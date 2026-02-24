//! FFI bridge to graph/bfs.cu via unified libgpu.a.
//! Accepts a `Csr` graph (see graph::csr) so callers never touch raw pointers.
//!
//! Variables:
//!   csr    : &Csr  — CSR graph built from adjacency list via Csr::from_adj
//!   source : usize — BFS start vertex
//!
//! Equation:
//!   level[v] = d  <=>  shortest-path distance source -> v equals d
//!   level[v] = -1      vertex v is unreachable from source

use super::csr::Csr;

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_bfs(row_ptr: *const i32, col_idx: *const i32, v: i32, e: i32, source: i32, level_out: *mut i32);
}

/// Run GPU BFS on a CSR graph. Returns level vector; -1 = unreachable.
#[cfg(feature = "cuda")]
pub fn bfs_gpu(csr: &Csr, source: usize) -> Vec<i32> {
    let mut level = vec![-1i32; csr.vertex_count()];
    unsafe {
        gpu_bfs(csr.row_ptr.as_ptr(), csr.col_idx.as_ptr(), csr.vertex_count() as i32, csr.edge_count() as i32, source as i32, level.as_mut_ptr());
    }
    level
}

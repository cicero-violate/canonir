//! Unified-memory CSR built on GPU via cudaMallocManaged.
//!
//! Variables:
//!   row_ptr : *mut i32  — unified pointer, length V+1, valid CPU+GPU
//!   col_idx : *mut i32  — unified pointer, length E,   valid CPU+GPU
//!   V, E    : usize     — vertex and edge counts
//!
//! Equations:
//!   row_ptr[v+1] - row_ptr[v] = out_degree(v)
//!   col_idx[row_ptr[v]..row_ptr[v+1]] = neighbours(v)
//!
//! Unlike Csr (host Vec), these pointers can be passed directly to any
//! CUDA kernel without cudaMemcpy. The driver page-migrates on first access.
//! cudaMemPrefetchAsync is called after build to warm CPU-side reads.

use super::adj_list::AdjList;

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_csr_build(adj_flat: *const i32, v: i32, e: i32, row_ptr_out: *mut *mut i32, col_idx_out: *mut *mut i32);
    fn gpu_csr_free(row_ptr: *mut i32, col_idx: *mut i32);
}

#[cfg(feature = "cuda")]
pub struct CsrUnified {
    pub row_ptr: *mut i32, // unified, length V+1
    pub col_idx: *mut i32, // unified, length E
    pub v: usize,
    pub e: usize,
}

#[cfg(feature = "cuda")]
impl CsrUnified {
    /// Build unified CSR from an AdjList on the GPU.
    pub fn from_adj(adj: &AdjList) -> Self {
        // Flatten adjacency list to (u,v) pairs
        let mut flat: Vec<i32> = Vec::with_capacity(adj.edge_count() * 2);
        for (u, neighbours) in adj.adj.iter().enumerate() {
            for &v in neighbours {
                flat.push(u as i32);
                flat.push(v as i32);
            }
        }
        let v = adj.vertex_count();
        let e = adj.edge_count();
        let mut row_ptr: *mut i32 = std::ptr::null_mut();
        let mut col_idx: *mut i32 = std::ptr::null_mut();
        unsafe {
            gpu_csr_build(flat.as_ptr(), v as i32, e as i32, &mut row_ptr, &mut col_idx);
        }
        Self { row_ptr, col_idx, v, e }
    }

    pub fn vertex_count(&self) -> usize {
        self.v
    }
    pub fn edge_count(&self) -> usize {
        self.e
    }

    /// Safe slice view of row_ptr (host-accessible via unified memory).
    pub fn row_ptr_slice(&self) -> &[i32] {
        unsafe { std::slice::from_raw_parts(self.row_ptr, self.v + 1) }
    }

    /// Safe slice view of col_idx.
    pub fn col_idx_slice(&self) -> &[i32] {
        unsafe { std::slice::from_raw_parts(self.col_idx, self.e) }
    }
}

#[cfg(feature = "cuda")]
impl Drop for CsrUnified {
    fn drop(&mut self) {
        unsafe {
            gpu_csr_free(self.row_ptr, self.col_idx);
        }
    }
}

// SAFETY: unified memory pointers are valid on any thread after
// cudaDeviceSynchronize; user must not alias across threads.
#[cfg(feature = "cuda")]
unsafe impl Send for CsrUnified {}

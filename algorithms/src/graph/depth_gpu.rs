//! GPU longest-path depth for DAG (iterative relaxation).

use super::csr::Csr;

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_longest_path_depth(row_ptr: *const i32, col_idx: *const i32, v: i32, depth_out: *mut i32);
}

#[cfg(feature = "cuda")]
pub fn longest_path_depth_gpu(csr: &Csr) -> Vec<i32> {
    let v = csr.vertex_count() as i32;
    let mut depth = vec![0i32; csr.vertex_count()];
    unsafe {
        gpu_longest_path_depth(
            csr.row_ptr.as_ptr(),
            csr.col_idx.as_ptr(),
            v,
            depth.as_mut_ptr(),
        );
    }
    depth
}

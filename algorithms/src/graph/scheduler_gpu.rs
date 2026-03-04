//! GPU ready-mask kernel for scheduler graphs.

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_ready_mask(
        status: *const u8,
        deps_offset: *const i32,
        deps_flat: *const i32,
        v: i32,
        ready_out: *mut u8,
        ready_count_out: *mut i32,
        completed_count_out: *mut i32,
    );
    fn gpu_pack_ready_priority(
        ready_mask: *const u8,
        priority: *const u16,
        v: i32,
        out_keys: *mut i64,
    );
}

/// Run GPU ready-mask kernel. Returns (ready_mask, ready_count, completed_count).
#[cfg(feature = "cuda")]
pub fn ready_mask_gpu(status: &[u8], deps_offset: &[i32], deps_flat: &[i32]) -> (Vec<u8>, i32, i32) {
    let v = status.len() as i32;
    let mut ready = vec![0u8; status.len()];
    let mut ready_count = 0i32;
    let mut completed_count = 0i32;
    unsafe {
        gpu_ready_mask(
            status.as_ptr(),
            deps_offset.as_ptr(),
            deps_flat.as_ptr(),
            v,
            ready.as_mut_ptr(),
            &mut ready_count,
            &mut completed_count,
        );
    }
    (ready, ready_count, completed_count)
}

/// Pack (priority, index) into keys for GPU sorting. key = (priority << 32) | index.
#[cfg(feature = "cuda")]
pub fn pack_ready_priority(ready_mask: &[u8], priority: &[u16]) -> Vec<i64> {
    let v = ready_mask.len() as i32;
    let mut keys = vec![-1i64; ready_mask.len()];
    unsafe {
        gpu_pack_ready_priority(
            ready_mask.as_ptr(),
            priority.as_ptr(),
            v,
            keys.as_mut_ptr(),
        );
    }
    keys
}

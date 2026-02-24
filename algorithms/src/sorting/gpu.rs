//! FFI bridge to sorting/bitonic_sort.cu via unified libgpu.a
//!
//! Variables:
//!   arr : *mut i64  — array to sort in-place, length N
//!   N   : i32       — must be a power of 2
//!
//! Equation:
//!   After gpu_bitonic_sort: arr[0] <= arr[1] <= ... <= arr[N-1]
//!   Complexity: O(log^2 N) kernel passes, O(N) parallel compare-swaps each

#[cfg(feature = "cuda")]
unsafe extern "C" {
    pub fn gpu_bitonic_sort(arr: *mut i64, n: i32);
}

/// Safe wrapper: sorts slice in-place on the GPU.
/// Pads to next power of 2 with i64::MAX, then trims.
#[cfg(feature = "cuda")]
pub fn bitonic_sort_gpu(arr: &mut Vec<i64>) {
    let orig_len = arr.len();
    let padded = orig_len.next_power_of_two();
    arr.resize(padded, i64::MAX);
    unsafe {
        gpu_bitonic_sort(arr.as_mut_ptr(), padded as i32);
    }
    arr.truncate(orig_len);
}

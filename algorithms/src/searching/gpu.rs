//! FFI bridge to searching/linear_search.cu via unified libgpu.a
//!
//! Variables:
//!   arr    : *const i64  — haystack, length N
//!   N      : i32         — array length
//!   target : i64         — value to find
//!   return : i32         — index of first match, or -1
//!
//! Equation:
//!   result = min { i | arr[i] == target }  via atomicMin across N threads
//!   Each thread: O(1), fully parallel

#[cfg(feature = "cuda")]
unsafe extern "C" {
    pub fn gpu_linear_search(arr: *const i64, n: i32, target: i64) -> i32;
}

/// Safe wrapper: returns Some(index) of first match, or None.
#[cfg(feature = "cuda")]
pub fn linear_search_gpu(arr: &[i64], target: i64) -> Option<usize> {
    let result = unsafe { gpu_linear_search(arr.as_ptr(), arr.len() as i32, target) };
    if result < 0 {
        None
    } else {
        Some(result as usize)
    }
}

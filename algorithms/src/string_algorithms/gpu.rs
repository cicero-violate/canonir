//! FFI bridge to string_algorithms/rabin_karp.cu via unified libgpu.a
//!
//! Variables:
//!   text, T      : *const u8, i32  — haystack and its length
//!   pattern, P   : *const u8, i32  — needle and its length
//!   matches_out  : *mut i32        — array of match start positions
//!   count_out    : *mut i32        — number of matches found
//!
//! Equation (polynomial rolling hash, one thread per window):
//!   H(pos) = (ph[pos+P] - ph[pos]*BASE^P) mod MOD
//!   match iff H(pos) == H(pattern)  + char-level verify

#[cfg(feature = "cuda")]
unsafe extern "C" {
    pub fn gpu_rabin_karp(text: *const u8, t: i32, pattern: *const u8, p: i32, matches_out: *mut i32, count_out: *mut i32);
}

/// Safe wrapper: returns sorted vec of all match start positions.
#[cfg(feature = "cuda")]
pub fn rabin_karp_gpu(text: &[u8], pattern: &[u8]) -> Vec<usize> {
    let max_matches = text.len();
    let mut matches = vec![0i32; max_matches];
    let mut count = 0i32;
    unsafe {
        gpu_rabin_karp(text.as_ptr(), text.len() as i32, pattern.as_ptr(), pattern.len() as i32, matches.as_mut_ptr(), &mut count);
    }
    let mut result: Vec<usize> = matches[..count as usize].iter().map(|&x| x as usize).collect();
    result.sort_unstable();
    result
}

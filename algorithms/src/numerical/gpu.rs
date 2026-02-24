//! FFI bridge to numerical/matrix_multiply.cu and numerical/sieve.cu
//!
//! Matrix multiply variables:
//!   A, B : *const i64  — input N×N matrices (row-major)
//!   C    : *mut i64    — output N×N matrix
//!   N    : i32         — matrix dimension
//!
//!   Equation: C[r][c] = sum_{k} A[r][k] * B[k][c]
//!   Tiled shared-memory reduces bandwidth by factor TILE=16
//!
//! Sieve variables:
//!   N          : i32    — upper bound
//!   primes_out : *mut i32 — output prime array (caller allocates N ints)
//!   count_out  : *mut i32 — number of primes found
//!
//!   Equation: primes_out = { p | 2 <= p <= N, p prime }

#[cfg(feature = "cuda")]
unsafe extern "C" {
    pub fn gpu_matrix_multiply(a: *const i64, b: *const i64, c: *mut i64, n: i32);
    pub fn gpu_sieve(n: i32, primes_out: *mut i32, count_out: *mut i32);
}

/// Safe wrapper for tiled GPU matrix multiply.
/// Inputs are flat row-major vecs of length N*N.
#[cfg(feature = "cuda")]
pub fn matrix_multiply_gpu(a: &[i64], b: &[i64], n: usize) -> Vec<i64> {
    let mut c = vec![0i64; n * n];
    unsafe {
        gpu_matrix_multiply(a.as_ptr(), b.as_ptr(), c.as_mut_ptr(), n as i32);
    }
    c
}

/// Safe wrapper for GPU sieve of Eratosthenes.
#[cfg(feature = "cuda")]
pub fn sieve_gpu(n: usize) -> Vec<i32> {
    let mut primes = vec![0i32; n];
    let mut count = 0i32;
    unsafe {
        gpu_sieve(n as i32, primes.as_mut_ptr(), &mut count);
    }
    primes.truncate(count as usize);
    primes
}

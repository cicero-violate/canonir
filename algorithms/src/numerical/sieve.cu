#include <cuda_runtime.h>
#include <stdlib.h>

// Variables:
//   N        = sieve upper bound
//   is_prime = boolean array length N+1, 1=prime 0=composite
//   p        = current sieve prime
//   tid      = thread id, each marks one multiple: k = p^2 + tid*p
//
// Equation:
//   is_prime[p^2 + tid*p] = 0   for all tid s.t. p^2 + tid*p <= N
//   Outer loop: p in { primes | p^2 <= N }
//   Total kernel launches = pi(sqrt(N))  ~= 2*sqrt(N)/ln(sqrt(N))

__global__ void sieve_kernel(int* is_prime, int p, int N) {
    int tid = blockIdx.x * blockDim.x + threadIdx.x;
    int k   = p * p + tid * p;
    if (k <= N) {
        is_prime[k] = 0;
    }
}

extern "C" void gpu_sieve(int N, int* primes_out, int* count_out) {
    size_t bytes = (size_t)(N + 1) * sizeof(int);

    int* h = (int*)malloc(bytes);
    for (int i = 0; i <= N; i++) {
        h[i] = (i >= 2) ? 1 : 0;
    }

    int* d;
    cudaMalloc(&d, bytes);
    cudaMemcpy(d, h, bytes, cudaMemcpyHostToDevice);

    int threads = 256;

    for (int p = 2; (long long)p * p <= N; p++) {
        if (!h[p]) continue;

        int mult = (N - p * p) / p + 1;
        if (mult <= 0) continue;

        int blocks = (mult + threads - 1) / threads;
        sieve_kernel<<<blocks, threads>>>(d, p, N);
        cudaDeviceSynchronize();

        cudaMemcpy(h, d, bytes, cudaMemcpyDeviceToHost);
    }

    int cnt = 0;
    for (int i = 2; i <= N; i++) {
        if (h[i]) {
            primes_out[cnt++] = i;
        }
    }

    *count_out = cnt;

    free(h);
    cudaFree(d);
}

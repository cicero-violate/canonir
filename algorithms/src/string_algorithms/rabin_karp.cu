#include <cuda_runtime.h>
#include <stdint.h>
#include <stdlib.h>

// Variables:
//   T, P     = text length, pattern length
//   BASE=31, MOD=1e9+7  = rolling hash parameters
//   ph[]     = prefix hash array of text, length T+1
//   pw[]     = power table pw[i] = BASE^i mod MOD
//   pat_hash = hash of pattern
//   pos      = thread id = window start position in text
//
// Equation (polynomial rolling hash):
//   ph[i] = sum_{k=0}^{i-1} text[k] * BASE^(i-1-k)  mod MOD
//   window_hash(pos) = (ph[pos+P] - ph[pos]*pw[P]) mod MOD
//   match iff window_hash(pos) == pat_hash  (+ char verify)

#define BASE 31ULL
#define MOD  1000000007ULL

__global__ void rk_kernel(
    const char* text, int T,
    const char* pat,  int P,
    const uint64_t* ph,
    const uint64_t* pw,
    uint64_t pat_hash,
    int* matches,
    int* count)
{
    int pos = blockIdx.x * blockDim.x + threadIdx.x;
    if (pos > T - P) return;

    uint64_t win =
        (ph[pos + P]
        + MOD
        - (ph[pos] * pw[P]) % MOD) % MOD;

    if (win != pat_hash) return;

    for (int i = 0; i < P; i++) {
        if (text[pos + i] != pat[i]) return;
    }

    int idx = atomicAdd(count, 1);
    matches[idx] = pos;
}

extern "C" void gpu_rabin_karp(
    const char* text, int T,
    const char* pattern, int P,
    int* matches_out,
    int* count_out)
{
    uint64_t* ph = (uint64_t*)malloc((T + 1) * sizeof(uint64_t));
    uint64_t* pw = (uint64_t*)malloc((T + 1) * sizeof(uint64_t));

    ph[0] = 0;
    pw[0] = 1;

    for (int i = 0; i < T; i++) {
        ph[i + 1] = (ph[i] * BASE + (uint8_t)text[i]) % MOD;
        pw[i + 1] = (pw[i] * BASE) % MOD;
    }

    uint64_t pat_hash = 0;
    for (int i = 0; i < P; i++) {
        pat_hash = (pat_hash * BASE + (uint8_t)pattern[i]) % MOD;
    }

    char *dt, *dp;
    uint64_t *dph, *dpw;
    int *dm, *dc;

    cudaMalloc(&dt,  T * sizeof(char));
    cudaMalloc(&dp,  P * sizeof(char));
    cudaMalloc(&dph, (T + 1) * sizeof(uint64_t));
    cudaMalloc(&dpw, (T + 1) * sizeof(uint64_t));
    cudaMalloc(&dm,  (T - P + 1) * sizeof(int));
    cudaMalloc(&dc,  sizeof(int));

    cudaMemcpy(dt,  text,    T * sizeof(char),         cudaMemcpyHostToDevice);
    cudaMemcpy(dp,  pattern, P * sizeof(char),         cudaMemcpyHostToDevice);
    cudaMemcpy(dph, ph,      (T + 1) * sizeof(uint64_t), cudaMemcpyHostToDevice);
    cudaMemcpy(dpw, pw,      (T + 1) * sizeof(uint64_t), cudaMemcpyHostToDevice);

    cudaMemset(dc, 0, sizeof(int));

    int threads = 256;
    int blocks  = (T - P + 1 + threads - 1) / threads;

    rk_kernel<<<blocks, threads>>>(
        dt, T,
        dp, P,
        dph, dpw,
        pat_hash,
        dm, dc);

    cudaDeviceSynchronize();

    cudaMemcpy(count_out, dc, sizeof(int), cudaMemcpyDeviceToHost);

    int hcount = *count_out;
    if (hcount > 0) {
        cudaMemcpy(matches_out, dm,
                   hcount * sizeof(int),
                   cudaMemcpyDeviceToHost);
    }

    free(ph);
    free(pw);

    cudaFree(dt);
    cudaFree(dp);
    cudaFree(dph);
    cudaFree(dpw);
    cudaFree(dm);
    cudaFree(dc);
}

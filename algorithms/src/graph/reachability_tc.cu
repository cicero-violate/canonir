#include <cuda_runtime.h>
#include <stdint.h>

__global__ void reachability_tc_init(
    unsigned long long* reach,
    int V,
    int W)
{
    int v = blockIdx.x * blockDim.x + threadIdx.x;
    if (v >= V) return;
    unsigned long long* row = reach + (size_t)v * (size_t)W;
    for (int w = 0; w < W; w++) {
        row[w] = 0ULL;
    }
    int word = v >> 6;
    int bit = v & 63;
    row[word] = 1ULL << bit;
}

__global__ void reachability_tc_relax(
    const int* row_ptr,
    const int* col_idx,
    int V,
    int W,
    unsigned long long* reach,
    int* changed)
{
    int u = blockIdx.x * blockDim.x + threadIdx.x;
    if (u >= V) return;
    const unsigned long long* src = reach + (size_t)u * (size_t)W;
    int start = row_ptr[u];
    int end = row_ptr[u + 1];
    for (int e = start; e < end; e++) {
        int v = col_idx[e];
        unsigned long long* dst = reach + (size_t)v * (size_t)W;
        for (int w = 0; w < W; w++) {
            unsigned long long val = src[w];
            if (val == 0ULL) continue;
            unsigned long long old = atomicOr(&dst[w], val);
            if ((old | val) != old) {
                *changed = 1;
            }
        }
    }
}

extern "C" void gpu_reachability_tc(
    const int* row_ptr,
    const int* col_idx,
    int V,
    int E,
    int W,
    int max_iters,
    unsigned long long* out)
{
    if (V <= 0 || E < 0 || W <= 0) return;

    size_t bytes_row = (size_t)(V + 1) * sizeof(int);
    size_t bytes_col = (size_t)E * sizeof(int);
    size_t bytes_reach = (size_t)V * (size_t)W * sizeof(unsigned long long);

    int *d_row, *d_col;
    unsigned long long* d_reach;
    int* d_changed;
    cudaMalloc(&d_row, bytes_row);
    cudaMalloc(&d_col, bytes_col);
    cudaMalloc(&d_reach, bytes_reach);
    cudaMalloc(&d_changed, sizeof(int));

    cudaMemcpy(d_row, row_ptr, bytes_row, cudaMemcpyHostToDevice);
    cudaMemcpy(d_col, col_idx, bytes_col, cudaMemcpyHostToDevice);

    int threads = 256;
    int blocks = (V + threads - 1) / threads;
    reachability_tc_init<<<blocks, threads>>>(d_reach, V, W);
    cudaDeviceSynchronize();

    for (int iter = 0; iter < max_iters; iter++) {
        int h_changed = 0;
        cudaMemcpy(d_changed, &h_changed, sizeof(int), cudaMemcpyHostToDevice);
        reachability_tc_relax<<<blocks, threads>>>(d_row, d_col, V, W, d_reach, d_changed);
        cudaDeviceSynchronize();
        cudaMemcpy(&h_changed, d_changed, sizeof(int), cudaMemcpyDeviceToHost);
        if (!h_changed) break;
    }

    cudaMemcpy(out, d_reach, bytes_reach, cudaMemcpyDeviceToHost);

    cudaFree(d_row);
    cudaFree(d_col);
    cudaFree(d_reach);
    cudaFree(d_changed);
}

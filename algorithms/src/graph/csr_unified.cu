#include <cuda_runtime.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>

// Unified Memory CSR builder.
//
// Variables:
//   V        = number of vertices
//   E        = number of edges
//   adj_flat = host array of (u, v) pairs, length E*2
//   row_ptr  = cudaMallocManaged, length V+1  (prefix sums of out-degree)
//   col_idx  = cudaMallocManaged, length E    (neighbour list)
//
// Equations:
//   row_ptr[0] = 0
//   row_ptr[v] = sum_{u=0}^{v-1} out_degree(u)    (exclusive prefix sum)
//   col_idx[row_ptr[u] .. row_ptr[u+1]] = neighbours(u)
//
// Prefix sum kernel (parallel, single block for simplicity):
//   scan[i] = sum_{j=0}^{i-1} degree[j]           (exclusive scan)
//
// Scatter kernel:
//   For each edge (u,v): col_idx[row_ptr[u] + local_offset[u]++] = v
//   (atomicAdd gives each edge a unique slot within its row)

// Degree count: one thread per edge, atomicAdd into degree[u]
__global__ void count_degrees(
    const int* adj_flat, int E, int* degree)
{
    int tid = blockIdx.x * blockDim.x + threadIdx.x;
    if (tid >= E) return;
    int u = adj_flat[tid * 2];
    atomicAdd(&degree[u], 1);
}

// Exclusive prefix sum (single block, handles up to 1024 vertices inline).
// For V > 1024 a multi-block scan would be needed.
__global__ void exclusive_scan(int* degree, int* row_ptr, int V) {
    extern __shared__ int tmp[];
    int tid = threadIdx.x;
    if (tid >= V + 1) return;
    tmp[tid] = (tid > 0 && tid <= V) ? degree[tid - 1] : 0;
    __syncthreads();
    for (int stride = 1; stride <= V; stride <<= 1) {
        int val = (tid >= stride) ? tmp[tid - stride] : 0;
        __syncthreads();
        tmp[tid] += val;
        __syncthreads();
    }
    row_ptr[tid] = tmp[tid];
}

// Scatter edges into col_idx using atomicAdd for slot assignment.
__global__ void scatter_edges(
    const int* adj_flat, int E,
    const int* row_ptr, int* cursor, int* col_idx)
{
    int tid = blockIdx.x * blockDim.x + threadIdx.x;
    if (tid >= E) return;
    int u = adj_flat[tid * 2 + 0];
    int v = adj_flat[tid * 2 + 1];
    int slot = atomicAdd(&cursor[u], 1);
    col_idx[slot] = v;
}

// Allocate and build a unified-memory CSR from a host edge list.
// row_ptr_out and col_idx_out are cudaMallocManaged — valid on CPU and GPU.
// Caller must cudaFree both when done.
extern "C" void gpu_csr_build(
    const int* adj_flat,   // host: E pairs (u,v)
    int V, int E,
    int** row_ptr_out,
    int** col_idx_out)
{
    // Unified allocations — accessible from both host and GPU
    int* row_ptr; cudaMallocManaged(&row_ptr, (V + 1) * sizeof(int));
    int* col_idx; cudaMallocManaged(&col_idx, E       * sizeof(int));
    int* degree;  cudaMallocManaged(&degree,  V       * sizeof(int));
    int* cursor;  cudaMallocManaged(&cursor,  V       * sizeof(int));

    // Copy edge list to device
    int* d_adj; cudaMalloc(&d_adj, E * 2 * sizeof(int));
    cudaMemcpy(d_adj, adj_flat, E * 2 * sizeof(int), cudaMemcpyHostToDevice);

    // Prefetch unified buffers to GPU before kernel launch
    int dev; cudaGetDevice(&dev);
    cudaMemset(degree, 0, V * sizeof(int));
    cudaMemset(cursor, 0, V * sizeof(int));
    cudaMemPrefetchAsync(degree,  V       * sizeof(int), dev, 0);
    cudaMemPrefetchAsync(cursor,  V       * sizeof(int), dev, 0);
    cudaMemPrefetchAsync(row_ptr, (V + 1) * sizeof(int), dev, 0);
    cudaMemPrefetchAsync(col_idx, E       * sizeof(int), dev, 0);

    int threads = 256;

    // Phase 1: count out-degrees
    count_degrees<<<(E + threads - 1) / threads, threads>>>(d_adj, E, degree);
    cudaDeviceSynchronize();

    // Phase 2: exclusive prefix sum -> row_ptr
    // Single block: supports V <= 1024; for larger V use multi-block scan
    int smem = (V + 1) * sizeof(int);
    exclusive_scan<<<1, V + 1, smem>>>(degree, row_ptr, V);
    cudaDeviceSynchronize();

    // After sync, unified row_ptr is CPU-readable — copy via host memcpy
    // then prefetch cursor back to GPU before scatter kernel
    // Prefetch row_ptr to CPU, copy to cursor on host, prefetch cursor to GPU
    cudaMemPrefetchAsync(row_ptr, (V + 1) * sizeof(int), cudaCpuDeviceId, 0);
    cudaDeviceSynchronize();
    for (int i = 0; i < V; i++) cursor[i] = row_ptr[i];
    cudaMemPrefetchAsync(cursor, V * sizeof(int), dev, 0);
    cudaMemPrefetchAsync(col_idx, E * sizeof(int), dev, 0);
    cudaDeviceSynchronize();

    // Phase 3: scatter edges into col_idx
    scatter_edges<<<(E + threads - 1) / threads, threads>>>(
        d_adj, E, row_ptr, cursor, col_idx);
    cudaDeviceSynchronize();

    // Prefetch results back to CPU so host can read row_ptr/col_idx directly
    cudaMemPrefetchAsync(row_ptr, (V + 1) * sizeof(int), cudaCpuDeviceId, 0);
    cudaMemPrefetchAsync(col_idx, E       * sizeof(int), cudaCpuDeviceId, 0);
    cudaDeviceSynchronize();

    cudaFree(d_adj);
    cudaFree(degree);
    cudaFree(cursor);

    *row_ptr_out = row_ptr;
    *col_idx_out = col_idx;
}

extern "C" void gpu_csr_free(int* row_ptr, int* col_idx) {
    cudaFree(row_ptr);
    cudaFree(col_idx);
}

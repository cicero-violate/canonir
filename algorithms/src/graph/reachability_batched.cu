#include <cuda_runtime.h>

// Variables:
//   row_ptr, col_idx : CSR graph (i32)
//   roots            : array of R root vertex indices
//   R                : number of roots
//   V                : number of vertices
//   out              : R x V matrix (i32), out[r*V + v] = 1 if v reachable from roots[r]
//
// One block per root. Threads within block cooperate on BFS via shared frontier.

#define MAX_FRONTIER 1024

__global__ void reachability_batched_kernel(
    const int* row_ptr,
    const int* col_idx,
    const int* roots,
    int R,
    int V,
    int* out)
{
    int r = blockIdx.x;
    if (r >= R) return;

    int* visited = out + r * V;

    __shared__ int frontier[MAX_FRONTIER];
    __shared__ int frontier_size;
    __shared__ int next_frontier[MAX_FRONTIER];
    __shared__ int next_size;

    if (threadIdx.x == 0) {
        int root = roots[r];
        frontier[0] = root;
        frontier_size = 1;
        visited[root] = 1;
    }
    __syncthreads();

    while (frontier_size > 0) {
        if (threadIdx.x == 0) next_size = 0;
        __syncthreads();

        for (int fi = threadIdx.x; fi < frontier_size; fi += blockDim.x) {
            int u = frontier[fi];
            int start = row_ptr[u];
            int end   = row_ptr[u + 1];
            for (int e = start; e < end; e++) {
                int v = col_idx[e];
                if (atomicCAS(&visited[v], 0, 1) == 0) {
                    int pos = atomicAdd(&next_size, 1);
                    if (pos < MAX_FRONTIER) {
                        next_frontier[pos] = v;
                    }
                }
            }
        }
        __syncthreads();

        int copy_count = min(next_size, MAX_FRONTIER);
        for (int i = threadIdx.x; i < copy_count; i += blockDim.x) {
            frontier[i] = next_frontier[i];
        }
        if (threadIdx.x == 0) frontier_size = copy_count;
        __syncthreads();
    }
}

extern "C" void gpu_reachability_batched(
    const int* row_ptr,
    const int* col_idx,
    int V,
    int E,
    const int* roots,
    int R,
    int* out)
{
    if (R <= 0 || V <= 0) return;

    size_t bytes_csr_row = (size_t)(V + 1) * sizeof(int);
    size_t bytes_csr_col = (size_t)E * sizeof(int);
    size_t bytes_roots   = (size_t)R * sizeof(int);
    size_t bytes_out     = (size_t)R * V * sizeof(int);

    int *d_row, *d_col, *d_roots, *d_out;
    cudaMalloc(&d_row,   bytes_csr_row);
    cudaMalloc(&d_col,   bytes_csr_col);
    cudaMalloc(&d_roots, bytes_roots);
    cudaMalloc(&d_out,   bytes_out);
    cudaMemset(d_out, 0, bytes_out);

    cudaMemcpy(d_row,   row_ptr, bytes_csr_row, cudaMemcpyHostToDevice);
    cudaMemcpy(d_col,   col_idx, bytes_csr_col, cudaMemcpyHostToDevice);
    cudaMemcpy(d_roots, roots,   bytes_roots,   cudaMemcpyHostToDevice);

    int threads = 128;
    reachability_batched_kernel<<<R, threads>>>(d_row, d_col, d_roots, R, V, d_out);
    cudaDeviceSynchronize();

    cudaMemcpy(out, d_out, bytes_out, cudaMemcpyDeviceToHost);

    cudaFree(d_row);
    cudaFree(d_col);
    cudaFree(d_roots);
    cudaFree(d_out);
}

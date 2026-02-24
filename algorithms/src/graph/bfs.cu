#include <cuda_runtime.h>
#include <stdint.h>

// Variables:
//   V        = number of vertices
//   E        = number of edges (CSR format)
//   row_ptr  = CSR row pointer array length V+1
//   col_idx  = CSR column index array length E
//   level    = output level array, -1 = unvisited
//   changed  = flag: did any thread update level this pass?
//
// Equation (frontier-parallel BFS):
//   level[v] = d  iff  shortest path from source to v = d
//   frontier_{d+1} = { u | (v,u) in E, v in frontier_d, level[u]==-1 }

__global__ void bfs_kernel(
    const int* row_ptr, const int* col_idx,
    int* level, int current_level, int V, int* changed)
{
    int v = blockIdx.x * blockDim.x + threadIdx.x;
    if (v >= V || level[v] != current_level) return;
    for (int e = row_ptr[v]; e < row_ptr[v + 1]; e++) {
        int u = col_idx[e];
        if (level[u] == -1) {
            level[u] = current_level + 1;
            *changed = 1;
        }
    }
}

extern "C" void gpu_bfs(const int* row_ptr, const int* col_idx,
                         int V, int E, int source, int* level_out)
{
    int *d_row, *d_col, *d_level, *d_changed;
    cudaMalloc(&d_row,     (V + 1) * sizeof(int));
    cudaMalloc(&d_col,     E       * sizeof(int));
    cudaMalloc(&d_level,   V       * sizeof(int));
    cudaMalloc(&d_changed, sizeof(int));
    cudaMemcpy(d_row, row_ptr, (V + 1) * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemcpy(d_col, col_idx, E       * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemset(d_level, -1, V * sizeof(int));
    int zero = 0;
    cudaMemcpy(d_level + source, &zero, sizeof(int), cudaMemcpyHostToDevice);
    int threads = 256, blocks = (V + 255) / 256;
    for (int d = 0; d < V; d++) {
        cudaMemset(d_changed, 0, sizeof(int));
        bfs_kernel<<<blocks, threads>>>(d_row, d_col, d_level, d, V, d_changed);
        int h = 0;
        cudaMemcpy(&h, d_changed, sizeof(int), cudaMemcpyDeviceToHost);
        if (!h) break;
    }
    cudaMemcpy(level_out, d_level, V * sizeof(int), cudaMemcpyDeviceToHost);
    cudaFree(d_row);
    cudaFree(d_col);
    cudaFree(d_level);
    cudaFree(d_changed);
}

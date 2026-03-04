#include <cuda_runtime.h>
#include <stdint.h>

__global__ void reach_init_kernel(int v, int* visited, int* frontier) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= v) return;
    visited[i] = 0;
    frontier[i] = 0;
}

__global__ void reach_seed_roots_kernel(const int* roots, int root_count, int* visited, int* frontier) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= root_count) return;
    int r = roots[i];
    if (r >= 0) {
        visited[r] = 1;
        frontier[r] = 1;
    }
}

__global__ void reach_clear_frontier_kernel(int v, int* frontier) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < v) frontier[i] = 0;
}

__global__ void reach_bfs_step_kernel(
    int v,
    const int* row_ptr,
    const int* col_idx,
    const int* frontier,
    int* next_frontier,
    int* visited,
    int* changed
) {
    int u = blockIdx.x * blockDim.x + threadIdx.x;
    if (u >= v) return;
    if (frontier[u] == 0) return;
    int start = row_ptr[u];
    int end = row_ptr[u + 1];
    for (int i = start; i < end; ++i) {
        int vtx = col_idx[i];
        if (vtx < 0 || vtx >= v) continue;
        if (atomicCAS(&visited[vtx], 0, 1) == 0) {
            next_frontier[vtx] = 1;
            *changed = 1;
        }
    }
}

extern "C" void gpu_reachability(
    const int* row_ptr,
    const int* col_idx,
    int v,
    int e,
    const int* roots,
    int root_count,
    int* visited_out
) {
    if (v <= 0 || e < 0) return;
    int threads = 256;
    int blocks = (v + threads - 1) / threads;
    int root_blocks = (root_count + threads - 1) / threads;

    int* d_row_ptr = nullptr;
    int* d_col_idx = nullptr;
    int* d_roots = nullptr;
    int* d_frontier = nullptr;
    int* d_next_frontier = nullptr;
    int* d_visited = nullptr;
    int* d_changed = nullptr;
    cudaMalloc(&d_row_ptr, sizeof(int) * (v + 1));
    cudaMalloc(&d_col_idx, sizeof(int) * e);
    cudaMalloc(&d_roots, sizeof(int) * root_count);
    cudaMalloc(&d_frontier, sizeof(int) * v);
    cudaMalloc(&d_next_frontier, sizeof(int) * v);
    cudaMalloc(&d_visited, sizeof(int) * v);
    cudaMalloc(&d_changed, sizeof(int));

    cudaMemcpy(d_row_ptr, row_ptr, sizeof(int) * (v + 1), cudaMemcpyHostToDevice);
    cudaMemcpy(d_col_idx, col_idx, sizeof(int) * e, cudaMemcpyHostToDevice);
    cudaMemcpy(d_roots, roots, sizeof(int) * root_count, cudaMemcpyHostToDevice);

    reach_init_kernel<<<blocks, threads>>>(v, d_visited, d_frontier);
    reach_clear_frontier_kernel<<<blocks, threads>>>(v, d_next_frontier);
    reach_seed_roots_kernel<<<root_blocks, threads>>>(d_roots, root_count, d_visited, d_frontier);

    for (;;) {
        int h_changed = 0;
        cudaMemcpy(d_changed, &h_changed, sizeof(int), cudaMemcpyHostToDevice);
        reach_clear_frontier_kernel<<<blocks, threads>>>(v, d_next_frontier);
        reach_bfs_step_kernel<<<blocks, threads>>>(v, d_row_ptr, d_col_idx, d_frontier, d_next_frontier, d_visited, d_changed);
        cudaMemcpy(&h_changed, d_changed, sizeof(int), cudaMemcpyDeviceToHost);
        if (h_changed == 0) break;
        int* tmp = d_frontier;
        d_frontier = d_next_frontier;
        d_next_frontier = tmp;
    }

    cudaMemcpy(visited_out, d_visited, sizeof(int) * v, cudaMemcpyDeviceToHost);
    cudaFree(d_row_ptr);
    cudaFree(d_col_idx);
    cudaFree(d_roots);
    cudaFree(d_frontier);
    cudaFree(d_next_frontier);
    cudaFree(d_visited);
    cudaFree(d_changed);
}

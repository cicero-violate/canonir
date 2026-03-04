#include <cuda_runtime.h>
#include <stdint.h>

__global__ void indegree_kernel(const int* row_ptr, const int* col_idx, int v, int* indegree) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= v) return;
    int start = row_ptr[i];
    int end = row_ptr[i + 1];
    for (int e = start; e < end; ++e) {
        int to = col_idx[e];
        atomicAdd(&indegree[to], 1);
    }
}

__global__ void frontier_kernel(const int* indegree, const uint8_t* removed, int v, uint8_t* frontier, int* count) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= v) return;
    if (!removed[i] && indegree[i] == 0) {
        frontier[i] = 1;
        atomicAdd(count, 1);
    } else {
        frontier[i] = 0;
    }
}

__global__ void remove_frontier_kernel(
    const int* row_ptr,
    const int* col_idx,
    int v,
    const uint8_t* frontier,
    int* indegree,
    uint8_t* removed
) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= v) return;
    if (!frontier[i]) return;
    removed[i] = 1;
    int start = row_ptr[i];
    int end = row_ptr[i + 1];
    for (int e = start; e < end; ++e) {
        int to = col_idx[e];
        atomicSub(&indegree[to], 1);
    }
}

extern "C" void gpu_topo_indegree(
    const int* row_ptr,
    const int* col_idx,
    int v,
    int* indegree_out
) {
    int* d_row = nullptr;
    int* d_col = nullptr;
    int* d_indegree = nullptr;
    cudaMalloc(&d_row, (v + 1) * sizeof(int));
    cudaMalloc(&d_col, row_ptr[v] * sizeof(int));
    cudaMalloc(&d_indegree, v * sizeof(int));
    cudaMemcpy(d_row, row_ptr, (v + 1) * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemcpy(d_col, col_idx, row_ptr[v] * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemset(d_indegree, 0, v * sizeof(int));

    int threads = 256;
    int blocks = (v + threads - 1) / threads;
    indegree_kernel<<<blocks, threads>>>(d_row, d_col, v, d_indegree);
    cudaDeviceSynchronize();

    cudaMemcpy(indegree_out, d_indegree, v * sizeof(int), cudaMemcpyDeviceToHost);
    cudaFree(d_row);
    cudaFree(d_col);
    cudaFree(d_indegree);
}

extern "C" void gpu_topo_frontier(
    const int* indegree,
    const uint8_t* removed,
    int v,
    uint8_t* frontier_out,
    int* count_out
) {
    int* d_indegree = nullptr;
    uint8_t* d_removed = nullptr;
    uint8_t* d_frontier = nullptr;
    int* d_count = nullptr;
    cudaMalloc(&d_indegree, v * sizeof(int));
    cudaMalloc(&d_removed, v * sizeof(uint8_t));
    cudaMalloc(&d_frontier, v * sizeof(uint8_t));
    cudaMalloc(&d_count, sizeof(int));
    cudaMemcpy(d_indegree, indegree, v * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemcpy(d_removed, removed, v * sizeof(uint8_t), cudaMemcpyHostToDevice);
    cudaMemset(d_frontier, 0, v * sizeof(uint8_t));
    cudaMemset(d_count, 0, sizeof(int));

    int threads = 256;
    int blocks = (v + threads - 1) / threads;
    frontier_kernel<<<blocks, threads>>>(d_indegree, d_removed, v, d_frontier, d_count);
    cudaDeviceSynchronize();

    cudaMemcpy(frontier_out, d_frontier, v * sizeof(uint8_t), cudaMemcpyDeviceToHost);
    cudaMemcpy(count_out, d_count, sizeof(int), cudaMemcpyDeviceToHost);
    cudaFree(d_indegree);
    cudaFree(d_removed);
    cudaFree(d_frontier);
    cudaFree(d_count);
}

extern "C" void gpu_topo_remove_frontier(
    const int* row_ptr,
    const int* col_idx,
    int v,
    const uint8_t* frontier,
    int* indegree,
    uint8_t* removed
) {
    int* d_row = nullptr;
    int* d_col = nullptr;
    uint8_t* d_frontier = nullptr;
    int* d_indegree = nullptr;
    uint8_t* d_removed = nullptr;
    cudaMalloc(&d_row, (v + 1) * sizeof(int));
    cudaMalloc(&d_col, row_ptr[v] * sizeof(int));
    cudaMalloc(&d_frontier, v * sizeof(uint8_t));
    cudaMalloc(&d_indegree, v * sizeof(int));
    cudaMalloc(&d_removed, v * sizeof(uint8_t));
    cudaMemcpy(d_row, row_ptr, (v + 1) * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemcpy(d_col, col_idx, row_ptr[v] * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemcpy(d_frontier, frontier, v * sizeof(uint8_t), cudaMemcpyHostToDevice);
    cudaMemcpy(d_indegree, indegree, v * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemcpy(d_removed, removed, v * sizeof(uint8_t), cudaMemcpyHostToDevice);

    int threads = 256;
    int blocks = (v + threads - 1) / threads;
    remove_frontier_kernel<<<blocks, threads>>>(d_row, d_col, v, d_frontier, d_indegree, d_removed);
    cudaDeviceSynchronize();

    cudaMemcpy(indegree, d_indegree, v * sizeof(int), cudaMemcpyDeviceToHost);
    cudaMemcpy(removed, d_removed, v * sizeof(uint8_t), cudaMemcpyDeviceToHost);
    cudaFree(d_row);
    cudaFree(d_col);
    cudaFree(d_frontier);
    cudaFree(d_indegree);
    cudaFree(d_removed);
}

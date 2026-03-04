#include <cuda_runtime.h>
#include <stdint.h>

__global__ void relax_edges_kernel(
    const int* row_ptr,
    const int* col_idx,
    int v,
    int* depth,
    int* changed
) {
    int u = blockIdx.x * blockDim.x + threadIdx.x;
    if (u >= v) return;
    int du = depth[u];
    int start = row_ptr[u];
    int end = row_ptr[u + 1];
    for (int e = start; e < end; ++e) {
        int vtx = col_idx[e];
        int cand = du + 1;
        int old = atomicMax(&depth[vtx], cand);
        if (cand > old) {
            atomicExch(changed, 1);
        }
    }
}

extern "C" void gpu_longest_path_depth(
    const int* row_ptr,
    const int* col_idx,
    int v,
    int* depth_out
) {
    if (v <= 0) return;
    int* d_row = nullptr;
    int* d_col = nullptr;
    int* d_depth = nullptr;
    int* d_changed = nullptr;
    cudaMalloc(&d_row, (v + 1) * sizeof(int));
    cudaMalloc(&d_col, row_ptr[v] * sizeof(int));
    cudaMalloc(&d_depth, v * sizeof(int));
    cudaMalloc(&d_changed, sizeof(int));

    cudaMemcpy(d_row, row_ptr, (v + 1) * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemcpy(d_col, col_idx, row_ptr[v] * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemset(d_depth, 0, v * sizeof(int));

    int threads = 256;
    int blocks = (v + threads - 1) / threads;
    for (int iter = 0; iter < v; ++iter) {
        cudaMemset(d_changed, 0, sizeof(int));
        relax_edges_kernel<<<blocks, threads>>>(d_row, d_col, v, d_depth, d_changed);
        cudaDeviceSynchronize();
        int changed = 0;
        cudaMemcpy(&changed, d_changed, sizeof(int), cudaMemcpyDeviceToHost);
        if (!changed) break;
    }

    cudaMemcpy(depth_out, d_depth, v * sizeof(int), cudaMemcpyDeviceToHost);
    cudaFree(d_row);
    cudaFree(d_col);
    cudaFree(d_depth);
    cudaFree(d_changed);
}

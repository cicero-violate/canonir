#include <cuda_runtime.h>
#include <stdint.h>

__global__ void ready_mask_kernel(
    const uint8_t* status,
    const int* deps_offset,
    const int* deps_flat,
    int v,
    uint8_t* ready_out,
    int* ready_count,
    int* completed_count
) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= v) return;

    uint8_t s = status[i];
    if (s == 3) { // Completed
        atomicAdd(completed_count, 1);
    }

    if (s != 0 && s != 1) { // not Pending/Ready
        ready_out[i] = 0;
        return;
    }

    int start = deps_offset[i];
    int end = deps_offset[i + 1];
    bool ok = true;
    for (int idx = start; idx < end; ++idx) {
        int dep = deps_flat[idx];
        if (status[dep] != 3) { // not Completed
            ok = false;
            break;
        }
    }
    ready_out[i] = ok ? 1 : 0;
    if (ok) {
        atomicAdd(ready_count, 1);
    }
}

extern "C" void gpu_ready_mask(
    const uint8_t* status,
    const int* deps_offset,
    const int* deps_flat,
    int v,
    uint8_t* ready_out,
    int* ready_count_out,
    int* completed_count_out
) {
    if (v <= 0) return;

    uint8_t* d_status = nullptr;
    int* d_deps_offset = nullptr;
    int* d_deps_flat = nullptr;
    uint8_t* d_ready = nullptr;
    int* d_ready_count = nullptr;
    int* d_completed_count = nullptr;

    cudaMalloc(&d_status, v * sizeof(uint8_t));
    cudaMalloc(&d_deps_offset, (v + 1) * sizeof(int));
    cudaMalloc(&d_deps_flat, deps_offset[v] * sizeof(int));
    cudaMalloc(&d_ready, v * sizeof(uint8_t));
    cudaMalloc(&d_ready_count, sizeof(int));
    cudaMalloc(&d_completed_count, sizeof(int));

    cudaMemcpy(d_status, status, v * sizeof(uint8_t), cudaMemcpyHostToDevice);
    cudaMemcpy(d_deps_offset, deps_offset, (v + 1) * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemcpy(d_deps_flat, deps_flat, deps_offset[v] * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemset(d_ready, 0, v * sizeof(uint8_t));
    cudaMemset(d_ready_count, 0, sizeof(int));
    cudaMemset(d_completed_count, 0, sizeof(int));

    int threads = 256;
    int blocks = (v + threads - 1) / threads;
    ready_mask_kernel<<<blocks, threads>>>(
        d_status,
        d_deps_offset,
        d_deps_flat,
        v,
        d_ready,
        d_ready_count,
        d_completed_count
    );
    cudaDeviceSynchronize();

    cudaMemcpy(ready_out, d_ready, v * sizeof(uint8_t), cudaMemcpyDeviceToHost);
    cudaMemcpy(ready_count_out, d_ready_count, sizeof(int), cudaMemcpyDeviceToHost);
    cudaMemcpy(completed_count_out, d_completed_count, sizeof(int), cudaMemcpyDeviceToHost);

    cudaFree(d_status);
    cudaFree(d_deps_offset);
    cudaFree(d_deps_flat);
    cudaFree(d_ready);
    cudaFree(d_ready_count);
    cudaFree(d_completed_count);
}

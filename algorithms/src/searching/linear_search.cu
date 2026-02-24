#include <cuda_runtime.h>
#include <stdint.h>

// Variables:
//   N      = array length
//   target = value sought
//   result = output index, initialised to N (sentinel = not found)
//   tid    = global thread id, each checks arr[tid] independently
//
// Equation:
//   result = min { i | arr[i] == target }  via atomicMin
//   Each thread: O(1) work, fully data-parallel across N threads

__global__ void linear_search_kernel(
    const int64_t* arr, int64_t target, int N, int* result)
{
    int tid = blockIdx.x * blockDim.x + threadIdx.x;
    if (tid < N && arr[tid] == target) {
        atomicMin(result, tid);
    }
}

extern "C" int gpu_linear_search(const int64_t* arr, int N, int64_t target) {
    int64_t* d_arr;
    int* d_res;

    cudaMalloc(&d_arr, N * sizeof(int64_t));
    cudaMalloc(&d_res, sizeof(int));

    cudaMemcpy(d_arr, arr, N * sizeof(int64_t), cudaMemcpyHostToDevice);
    cudaMemcpy(d_res, &N, sizeof(int), cudaMemcpyHostToDevice);

    int threads = 256;
    int blocks  = (N + threads - 1) / threads;

    linear_search_kernel<<<blocks, threads>>>(d_arr, target, N, d_res);
    cudaDeviceSynchronize();

    int h;
    cudaMemcpy(&h, d_res, sizeof(int), cudaMemcpyDeviceToHost);

    cudaFree(d_arr);
    cudaFree(d_res);

    return (h == N) ? -1 : h;
}

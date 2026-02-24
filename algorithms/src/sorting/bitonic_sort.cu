#include <cuda_runtime.h>
#include <stdint.h>

// Variables:
//   N   = array length (power of 2)
//   k   = bitonic sequence length (outer pass, doubles each step)
//   j   = comparison stride (inner pass, halves each step)
//   tid = global thread id
//   ixj = partner index = tid XOR j
//
// Equation:
//   ascending  iff (tid & k) == 0
//   swap(tid, ixj) if ascending ? arr[tid] > arr[ixj] : arr[tid] < arr[ixj]
//   Complexity: O(log^2 N) passes, O(N) parallel comparisons per pass

__global__ void bitonic_step(int64_t* arr, int j, int k) {
    int tid = blockIdx.x * blockDim.x + threadIdx.x;
    int ixj = tid ^ j;
    if (tid >= gridDim.x * blockDim.x) return;
    if (ixj <= tid) return;
    bool asc = (tid & k) == 0;
    if (asc ? arr[tid] > arr[ixj] : arr[tid] < arr[ixj]) {
        int64_t t = arr[tid];
        arr[tid] = arr[ixj];
        arr[ixj] = t;
    }
}

extern "C" void gpu_bitonic_sort(int64_t* arr, int N) {
    int64_t* d;
    cudaMalloc(&d, N * sizeof(int64_t));
    cudaMemcpy(d, arr, N * sizeof(int64_t), cudaMemcpyHostToDevice);

    int threads = 256;
    int blocks  = (N + threads - 1) / threads;

    for (int k = 2; k <= N; k <<= 1) {
        for (int j = k >> 1; j > 0; j >>= 1) {
            bitonic_step<<<blocks, threads>>>(d, j, k);
            cudaDeviceSynchronize();
        }
    }

    cudaMemcpy(arr, d, N * sizeof(int64_t), cudaMemcpyDeviceToHost);
    cudaFree(d);
}

#include <cuda_runtime.h>
#include <stdint.h>

// Variables:
//   N    = matrix dimension (N x N square)
//   TILE = shared-memory tile width (16)
//   sA, sB = shared memory tiles for A and B
//   row, col = output element this thread computes
//   acc  = running dot-product accumulator
//
// Equation:
//   C[row][col] = sum_{k=0}^{N-1} A[row][k] * B[k][col]
//   Tiled: each block loads TILE x TILE submatrices into shared mem,
//          reducing global mem bandwidth by factor TILE

#define TILE 16

__global__ void matmul_kernel(
    const int64_t* A, const int64_t* B, int64_t* C, int N)
{
    __shared__ int64_t sA[TILE][TILE];
    __shared__ int64_t sB[TILE][TILE];

    int row = blockIdx.y * TILE + threadIdx.y;
    int col = blockIdx.x * TILE + threadIdx.x;

    int64_t acc = 0;

    int tiles = (N + TILE - 1) / TILE;
    for (int t = 0; t < tiles; t++) {
        int ac = t * TILE + threadIdx.x;
        int br = t * TILE + threadIdx.y;

        sA[threadIdx.y][threadIdx.x] =
            (row < N && ac < N) ? A[row * N + ac] : 0;

        sB[threadIdx.y][threadIdx.x] =
            (br < N && col < N) ? B[br * N + col] : 0;

        __syncthreads();

        for (int k = 0; k < TILE; k++) {
            acc += sA[threadIdx.y][k] * sB[k][threadIdx.x];
        }

        __syncthreads();
    }

    if (row < N && col < N) {
        C[row * N + col] = acc;
    }
}

extern "C" void gpu_matrix_multiply(
    const int64_t* A,
    const int64_t* B,
    int64_t* C,
    int N)
{
    size_t bytes = (size_t)N * N * sizeof(int64_t);

    int64_t *dA, *dB, *dC;
    cudaMalloc(&dA, bytes);
    cudaMalloc(&dB, bytes);
    cudaMalloc(&dC, bytes);

    cudaMemcpy(dA, A, bytes, cudaMemcpyHostToDevice);
    cudaMemcpy(dB, B, bytes, cudaMemcpyHostToDevice);

    dim3 block(TILE, TILE);
    dim3 grid((N + TILE - 1) / TILE,
              (N + TILE - 1) / TILE);

    matmul_kernel<<<grid, block>>>(dA, dB, dC, N);
    cudaDeviceSynchronize();

    cudaMemcpy(C, dC, bytes, cudaMemcpyDeviceToHost);

    cudaFree(dA);
    cudaFree(dB);
    cudaFree(dC);
}

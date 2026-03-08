#include <cuda_runtime.h>
#include <math.h>

#define TILE 16

__global__ void cosine_distance_kernel(
    const float* phi,
    float* out,
    int m,
    int k)
{
    int j = blockIdx.x * TILE + threadIdx.x;
    int i = blockIdx.y * TILE + threadIdx.y;

    if (i >= m || j >= m) {
        return;
    }

    int base_i = i * k;
    int base_j = j * k;

    float dot = 0.0f;
    float norm_i = 0.0f;
    float norm_j = 0.0f;

    for (int t = 0; t < k; t++) {
        float a = phi[base_i + t];
        float b = phi[base_j + t];
        dot += a * b;
        norm_i += a * a;
        norm_j += b * b;
    }

    float denom = sqrtf(norm_i) * sqrtf(norm_j);
    float cos_sim = (denom > 0.0f) ? (dot / denom) : 0.0f;
    out[i * m + j] = 1.0f - cos_sim;
}

extern "C" void gpu_cosine_distance(
    const float* phi,
    float* out,
    int m,
    int k)
{
    if (m <= 0 || k <= 0) {
        return;
    }

    size_t phi_bytes = (size_t)m * (size_t)k * sizeof(float);
    size_t out_bytes = (size_t)m * (size_t)m * sizeof(float);

    float *d_phi, *d_out;
    cudaMalloc(&d_phi, phi_bytes);
    cudaMalloc(&d_out, out_bytes);

    cudaMemcpy(d_phi, phi, phi_bytes, cudaMemcpyHostToDevice);

    dim3 block(TILE, TILE);
    dim3 grid((m + TILE - 1) / TILE,
              (m + TILE - 1) / TILE);

    cosine_distance_kernel<<<grid, block>>>(d_phi, d_out, m, k);
    cudaDeviceSynchronize();

    cudaMemcpy(out, d_out, out_bytes, cudaMemcpyDeviceToHost);

    cudaFree(d_phi);
    cudaFree(d_out);
}

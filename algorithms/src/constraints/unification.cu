#include <cuda_runtime.h>
#include <stdint.h>

__device__ __forceinline__ int uf_find_root(int* parent, int x) {
    int p = parent[x];
    while (p != x) {
        int gp = parent[p];
        parent[x] = gp;
        x = p;
        p = gp;
    }
    return x;
}

__global__ void uf_init_parent(int n, int* parent) {
    int tid = blockIdx.x * blockDim.x + threadIdx.x;
    if (tid < n) {
        parent[tid] = tid;
    }
}

__global__ void uf_union_edges(int edge_count, const int* edge_u, const int* edge_v, int* parent, int* changed) {
    int tid = blockIdx.x * blockDim.x + threadIdx.x;
    if (tid >= edge_count) {
        return;
    }

    int a = edge_u[tid];
    int b = edge_v[tid];
    if (a < 0 || b < 0) {
        return;
    }

    int ra = uf_find_root(parent, a);
    int rb = uf_find_root(parent, b);

    while (ra != rb) {
        int hi = ra > rb ? ra : rb;
        int lo = ra > rb ? rb : ra;

        int prev = atomicCAS(&parent[hi], hi, lo);
        if (prev == hi) {
            *changed = 1;
            break;
        }

        ra = uf_find_root(parent, prev);
        rb = uf_find_root(parent, lo);
    }
}

__global__ void uf_compress_paths(int n, int* parent) {
    int tid = blockIdx.x * blockDim.x + threadIdx.x;
    if (tid >= n) {
        return;
    }

    int x = tid;
    int root = uf_find_root(parent, x);
    while (parent[x] != x) {
        int next = parent[x];
        parent[x] = root;
        x = next;
    }
    parent[tid] = root;
}

extern "C" int gpu_union_find_solve(
    int var_count,
    int edge_count,
    const int* edge_u,
    const int* edge_v,
    int* parent_out
) {
    if (var_count < 0 || edge_count < 0 || parent_out == nullptr) {
        return 0;
    }

    if (var_count == 0) {
        return 1;
    }

    int* d_parent = nullptr;
    int* d_u = nullptr;
    int* d_v = nullptr;
    int* d_changed = nullptr;

    cudaError_t err = cudaSuccess;
    err = cudaMalloc(&d_parent, sizeof(int) * var_count);
    if (err != cudaSuccess) return 0;
    err = cudaMalloc(&d_changed, sizeof(int));
    if (err != cudaSuccess) {
        cudaFree(d_parent);
        return 0;
    }

    if (edge_count > 0) {
        err = cudaMalloc(&d_u, sizeof(int) * edge_count);
        if (err != cudaSuccess) {
            cudaFree(d_parent); cudaFree(d_changed);
            return 0;
        }
        err = cudaMalloc(&d_v, sizeof(int) * edge_count);
        if (err != cudaSuccess) {
            cudaFree(d_parent); cudaFree(d_changed); cudaFree(d_u);
            return 0;
        }
        cudaMemcpy(d_u, edge_u, sizeof(int) * edge_count, cudaMemcpyHostToDevice);
        cudaMemcpy(d_v, edge_v, sizeof(int) * edge_count, cudaMemcpyHostToDevice);
    }

    const int threads = 256;
    int blocks_nodes = (var_count + threads - 1) / threads;
    uf_init_parent<<<blocks_nodes, threads>>>(var_count, d_parent);

    if (edge_count > 0) {
        int blocks_edges = (edge_count + threads - 1) / threads;
        int max_iters = var_count > 0 ? var_count : 1;
        for (int it = 0; it < max_iters; ++it) {
            int h_changed = 0;
            cudaMemcpy(d_changed, &h_changed, sizeof(int), cudaMemcpyHostToDevice);
            uf_union_edges<<<blocks_edges, threads>>>(edge_count, d_u, d_v, d_parent, d_changed);
            uf_compress_paths<<<blocks_nodes, threads>>>(var_count, d_parent);
            cudaMemcpy(&h_changed, d_changed, sizeof(int), cudaMemcpyDeviceToHost);
            if (h_changed == 0) {
                break;
            }
        }
    } else {
        uf_compress_paths<<<blocks_nodes, threads>>>(var_count, d_parent);
    }

    err = cudaDeviceSynchronize();
    if (err != cudaSuccess) {
        cudaFree(d_parent); cudaFree(d_changed);
        if (d_u) cudaFree(d_u);
        if (d_v) cudaFree(d_v);
        return 0;
    }

    cudaMemcpy(parent_out, d_parent, sizeof(int) * var_count, cudaMemcpyDeviceToHost);

    cudaFree(d_parent);
    cudaFree(d_changed);
    if (d_u) cudaFree(d_u);
    if (d_v) cudaFree(d_v);

    return 1;
}

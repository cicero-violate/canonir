#include <cuda_runtime.h>
#include <stdint.h>
#include <limits.h>

// Push-relabel with parallel discharge and relabel.
// Graph is represented as edge list converted to CSR of edge indices.

__device__ __forceinline__ int64_t atomicAdd_i64(int64_t* addr, int64_t val) {
    return (int64_t)atomicAdd((unsigned long long*)addr, (unsigned long long)val);
}

__global__ void init_preflow_kernel(
    int v,
    int source,
    const int* row_ptr,
    const int* col_idx,
    const int* edge_to,
    const int* edge_rev,
    int64_t* edge_cap,
    int* height,
    int64_t* excess
) {
    if (threadIdx.x == 0 && blockIdx.x == 0) {
        for (int i = 0; i < v; ++i) {
            height[i] = 0;
            excess[i] = 0;
        }
        height[source] = v;
        int start = row_ptr[source];
        int end = row_ptr[source + 1];
        for (int i = start; i < end; ++i) {
            int eidx = col_idx[i];
            int to = edge_to[eidx];
            int64_t c = edge_cap[eidx];
            if (c > 0) {
                edge_cap[eidx] = 0;
                int rev = edge_rev[eidx];
                edge_cap[rev] += c;
                excess[to] += c;
            }
        }
    }
}

__global__ void push_kernel(
    int v,
    int source,
    int sink,
    const int* row_ptr,
    const int* col_idx,
    const int* edge_to,
    const int* edge_rev,
    int64_t* edge_cap,
    int* height,
    int64_t* excess,
    int* progress
) {
    int u = blockIdx.x * blockDim.x + threadIdx.x;
    if (u >= v || u == source || u == sink) return;
    int64_t ex = excess[u];
    if (ex <= 0) return;

    int start = row_ptr[u];
    int end = row_ptr[u + 1];
    for (int i = start; i < end && ex > 0; ++i) {
        int eidx = col_idx[i];
        int vtx = edge_to[eidx];
        int64_t cap = edge_cap[eidx];
        if (cap <= 0) continue;
        int hu = height[u];
        int hv = height[vtx];
        if (hu == hv + 1) {
            int64_t delta = ex < cap ? ex : cap;
            if (delta > 0) {
                atomicAdd_i64(&edge_cap[eidx], -delta);
                int rev = edge_rev[eidx];
                atomicAdd_i64(&edge_cap[rev], delta);
                atomicAdd_i64(&excess[u], -delta);
                atomicAdd_i64(&excess[vtx], delta);
                ex -= delta;
                *progress = 1;
            }
        }
    }
}

__global__ void relabel_kernel(
    int v,
    int source,
    int sink,
    const int* row_ptr,
    const int* col_idx,
    const int* edge_to,
    int64_t* edge_cap,
    int* height,
    int64_t* excess,
    int* progress
) {
    int u = blockIdx.x * blockDim.x + threadIdx.x;
    if (u >= v || u == source || u == sink) return;
    if (excess[u] <= 0) return;

    int start = row_ptr[u];
    int end = row_ptr[u + 1];
    int min_h = INT_MAX;
    for (int i = start; i < end; ++i) {
        int eidx = col_idx[i];
        if (edge_cap[eidx] > 0) {
            int vtx = edge_to[eidx];
            int hv = height[vtx];
            if (hv < min_h) min_h = hv;
        }
    }
    if (min_h != INT_MAX) {
        height[u] = min_h + 1;
        *progress = 1;
    }
}

__global__ void init_levels_kernel(int v, int sink, int* level, int* frontier) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= v) return;
    level[i] = -1;
    frontier[i] = 0;
    if (i == sink) {
        level[i] = 0;
        frontier[i] = 1;
    }
}

__global__ void bfs_expand_kernel(
    int v,
    const int* in_row_ptr,
    const int* in_col_idx,
    const int* edge_from,
    const int64_t* edge_cap,
    const int* level,
    const int* frontier,
    int* next_frontier,
    int* changed
) {
    int u = blockIdx.x * blockDim.x + threadIdx.x;
    if (u >= v) return;
    if (frontier[u] == 0) return;
    int base = level[u];
    int start = in_row_ptr[u];
    int end = in_row_ptr[u + 1];
    for (int i = start; i < end; ++i) {
        int eidx = in_col_idx[i];
        if (edge_cap[eidx] <= 0) continue;
        int vtx = edge_from[eidx];
        if (vtx < 0 || vtx >= v) continue;
        if (atomicCAS(&((int*)level)[vtx], -1, base + 1) == -1) {
            next_frontier[vtx] = 1;
            *changed = 1;
        }
    }
}

__global__ void mf_clear_frontier_kernel(int v, int* frontier) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < v) frontier[i] = 0;
}

__global__ void finalize_height_kernel(int v, int source, const int* level, int* height) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= v) return;
    if (i == source) {
        height[i] = v;
    } else {
        int lv = level[i];
        height[i] = (lv >= 0) ? lv : (v + 1);
    }
}

extern "C" int64_t gpu_max_flow_push_relabel(
    int v,
    int e,
    const int* src,
    const int* dst,
    const int64_t* cap,
    int source,
    int sink
) {
    if (v <= 0 || source < 0 || sink < 0 || source >= v || sink >= v || source == sink) {
        return 0;
    }

    // Build edge list with reverse edges
    int edges = e * 2;
    int* h_edge_to = (int*)malloc(sizeof(int) * edges);
    int* h_edge_from = (int*)malloc(sizeof(int) * edges);
    int* h_edge_rev = (int*)malloc(sizeof(int) * edges);
    int64_t* h_edge_cap = (int64_t*)malloc(sizeof(int64_t) * edges);
    int* out_deg = (int*)calloc((size_t)v, sizeof(int));
    if (!h_edge_to || !h_edge_from || !h_edge_rev || !h_edge_cap || !out_deg) {
        if (h_edge_to) free(h_edge_to);
        if (h_edge_from) free(h_edge_from);
        if (h_edge_rev) free(h_edge_rev);
        if (h_edge_cap) free(h_edge_cap);
        if (out_deg) free(out_deg);
        return 0;
    }

    int edge_idx = 0;
    for (int i = 0; i < e; ++i) {
        int u = src[i];
        int vv = dst[i];
        int64_t c = cap[i];
        if (u < 0 || vv < 0 || u >= v || vv >= v || c <= 0) continue;
        int fwd = edge_idx;
        int rev = edge_idx + 1;
        h_edge_to[fwd] = vv;
        h_edge_from[fwd] = u;
        h_edge_rev[fwd] = rev;
        h_edge_cap[fwd] = c;
        h_edge_to[rev] = u;
        h_edge_from[rev] = vv;
        h_edge_rev[rev] = fwd;
        h_edge_cap[rev] = 0;
        out_deg[u] += 1;
        out_deg[vv] += 1;
        edge_idx += 2;
    }
    edges = edge_idx;

    // Build CSR over edge indices (outgoing)
    int* h_row_ptr = (int*)malloc(sizeof(int) * (size_t)(v + 1));
    int* h_col_idx = (int*)malloc(sizeof(int) * (size_t)edges);
    if (!h_row_ptr || !h_col_idx) {
        free(h_edge_to); free(h_edge_from); free(h_edge_rev); free(h_edge_cap); free(out_deg);
        if (h_row_ptr) free(h_row_ptr);
        if (h_col_idx) free(h_col_idx);
        return 0;
    }
    h_row_ptr[0] = 0;
    for (int i = 0; i < v; ++i) {
        h_row_ptr[i + 1] = h_row_ptr[i] + out_deg[i];
        out_deg[i] = 0;
    }
    for (int i = 0; i < edges; ++i) {
        int u = h_edge_from[i];
        int pos = h_row_ptr[u] + out_deg[u]++;
        h_col_idx[pos] = i;
    }

    // Build CSR over incoming edge indices
    int* in_deg = (int*)calloc((size_t)v, sizeof(int));
    int* h_in_row_ptr = (int*)malloc(sizeof(int) * (size_t)(v + 1));
    int* h_in_col_idx = (int*)malloc(sizeof(int) * (size_t)edges);
    if (!in_deg || !h_in_row_ptr || !h_in_col_idx) {
        free(h_edge_to); free(h_edge_from); free(h_edge_rev); free(h_edge_cap); free(out_deg);
        if (in_deg) free(in_deg);
        if (h_in_row_ptr) free(h_in_row_ptr);
        if (h_in_col_idx) free(h_in_col_idx);
        return 0;
    }
    for (int i = 0; i < edges; ++i) {
        int to = h_edge_to[i];
        in_deg[to] += 1;
    }
    h_in_row_ptr[0] = 0;
    for (int i = 0; i < v; ++i) {
        h_in_row_ptr[i + 1] = h_in_row_ptr[i] + in_deg[i];
        in_deg[i] = 0;
    }
    for (int i = 0; i < edges; ++i) {
        int to = h_edge_to[i];
        int pos = h_in_row_ptr[to] + in_deg[to]++;
        h_in_col_idx[pos] = i;
    }

    // Device allocations
    int *d_row_ptr, *d_col_idx, *d_in_row_ptr, *d_in_col_idx, *d_edge_to, *d_edge_from, *d_edge_rev, *d_height, *d_progress;
    int64_t *d_edge_cap, *d_excess;
    int *d_level, *d_frontier, *d_next_frontier, *d_changed;
    cudaMalloc(&d_row_ptr, sizeof(int) * (v + 1));
    cudaMalloc(&d_col_idx, sizeof(int) * edges);
    cudaMalloc(&d_in_row_ptr, sizeof(int) * (v + 1));
    cudaMalloc(&d_in_col_idx, sizeof(int) * edges);
    cudaMalloc(&d_edge_to, sizeof(int) * edges);
    cudaMalloc(&d_edge_from, sizeof(int) * edges);
    cudaMalloc(&d_edge_rev, sizeof(int) * edges);
    cudaMalloc(&d_edge_cap, sizeof(int64_t) * edges);
    cudaMalloc(&d_height, sizeof(int) * v);
    cudaMalloc(&d_excess, sizeof(int64_t) * v);
    cudaMalloc(&d_progress, sizeof(int));
    cudaMalloc(&d_level, sizeof(int) * v);
    cudaMalloc(&d_frontier, sizeof(int) * v);
    cudaMalloc(&d_next_frontier, sizeof(int) * v);
    cudaMalloc(&d_changed, sizeof(int));

    cudaMemcpy(d_row_ptr, h_row_ptr, sizeof(int) * (v + 1), cudaMemcpyHostToDevice);
    cudaMemcpy(d_col_idx, h_col_idx, sizeof(int) * edges, cudaMemcpyHostToDevice);
    cudaMemcpy(d_in_row_ptr, h_in_row_ptr, sizeof(int) * (v + 1), cudaMemcpyHostToDevice);
    cudaMemcpy(d_in_col_idx, h_in_col_idx, sizeof(int) * edges, cudaMemcpyHostToDevice);
    cudaMemcpy(d_edge_to, h_edge_to, sizeof(int) * edges, cudaMemcpyHostToDevice);
    cudaMemcpy(d_edge_from, h_edge_from, sizeof(int) * edges, cudaMemcpyHostToDevice);
    cudaMemcpy(d_edge_rev, h_edge_rev, sizeof(int) * edges, cudaMemcpyHostToDevice);
    cudaMemcpy(d_edge_cap, h_edge_cap, sizeof(int64_t) * edges, cudaMemcpyHostToDevice);

    init_preflow_kernel<<<1, 1>>>(v, source, d_row_ptr, d_col_idx, d_edge_to, d_edge_rev, d_edge_cap, d_height, d_excess);
    cudaDeviceSynchronize();

    int threads = 256;
    int blocks = (v + threads - 1) / threads;
    int max_iters = v * 4 + 64;
    for (int iter = 0; iter < max_iters; ++iter) {
        if (iter % (v + 1) == 0) {
            // Global relabel from sink over residual graph.
            init_levels_kernel<<<blocks, threads>>>(v, sink, d_level, d_frontier);
            cudaDeviceSynchronize();
            for (;;) {
                int h_changed = 0;
                cudaMemcpy(d_changed, &h_changed, sizeof(int), cudaMemcpyHostToDevice);
            mf_clear_frontier_kernel<<<blocks, threads>>>(v, d_next_frontier);
                bfs_expand_kernel<<<blocks, threads>>>(
                    v, d_in_row_ptr, d_in_col_idx, d_edge_from, d_edge_cap,
                    d_level, d_frontier, d_next_frontier, d_changed
                );
                cudaMemcpy(&h_changed, d_changed, sizeof(int), cudaMemcpyDeviceToHost);
                if (h_changed == 0) break;
                // swap frontier buffers
                int* tmp = d_frontier;
                d_frontier = d_next_frontier;
                d_next_frontier = tmp;
            }
            finalize_height_kernel<<<blocks, threads>>>(v, source, d_level, d_height);
            cudaDeviceSynchronize();
        }
        int h_progress = 0;
        cudaMemcpy(d_progress, &h_progress, sizeof(int), cudaMemcpyHostToDevice);
        push_kernel<<<blocks, threads>>>(v, source, sink, d_row_ptr, d_col_idx, d_edge_to, d_edge_rev, d_edge_cap, d_height, d_excess, d_progress);
        relabel_kernel<<<blocks, threads>>>(v, source, sink, d_row_ptr, d_col_idx, d_edge_to, d_edge_cap, d_height, d_excess, d_progress);
        cudaMemcpy(&h_progress, d_progress, sizeof(int), cudaMemcpyDeviceToHost);
        if (h_progress == 0) break;
    }

    int64_t result = 0;
    cudaMemcpy(&result, d_excess + sink, sizeof(int64_t), cudaMemcpyDeviceToHost);

    cudaFree(d_row_ptr); cudaFree(d_col_idx); cudaFree(d_in_row_ptr); cudaFree(d_in_col_idx);
    cudaFree(d_edge_to); cudaFree(d_edge_from); cudaFree(d_edge_rev);
    cudaFree(d_edge_cap); cudaFree(d_height); cudaFree(d_excess); cudaFree(d_progress);
    cudaFree(d_level); cudaFree(d_frontier); cudaFree(d_next_frontier); cudaFree(d_changed);
    free(h_edge_to); free(h_edge_from); free(h_edge_rev); free(h_edge_cap);
    free(h_row_ptr); free(h_col_idx); free(out_deg);
    free(h_in_row_ptr); free(h_in_col_idx); free(in_deg);
    return result;
}

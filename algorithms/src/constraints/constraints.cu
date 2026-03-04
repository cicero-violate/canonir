#include <cuda_runtime.h>
#include <stdint.h>

// GPU kernels for AC-3 and forward checking.
//
// Layout assumptions:
// - domains are stored as contiguous indices 0..len-1 per variable
// - domain_offsets: length (var_count+1), offsets into domain_active
// - domain_active: int array, 1=active, 0=removed
// - arcs: arrays arc_i, arc_j, arc_dom_i_len, arc_dom_j_len, arc_constraint_offset
// - constraint_values: flattened matrix for each arc, size dom_i_len * dom_j_len, 1=allowed

__global__ void ac3_revise_kernel(
    int arc_count,
    const int* arc_i,
    const int* arc_j,
    const int* arc_dom_i_len,
    const int* arc_dom_j_len,
    const int* arc_constraint_offset,
    const int* domain_offsets,
    int* domain_active,
    const uint8_t* constraint_values,
    int* changed
) {
    int arc = blockIdx.x;
    int xi = threadIdx.x;
    if (arc >= arc_count) return;
    int di_len = arc_dom_i_len[arc];
    int dj_len = arc_dom_j_len[arc];
    if (xi >= di_len) return;
    int var_i = arc_i[arc];
    int var_j = arc_j[arc];
    int di_base = domain_offsets[var_i];
    int dj_base = domain_offsets[var_j];
    int idx_i = di_base + xi;
    if (domain_active[idx_i] == 0) return;
    int c_off = arc_constraint_offset[arc];
    bool supported = false;
    for (int y = 0; y < dj_len; ++y) {
        int idx_j = dj_base + y;
        if (domain_active[idx_j] == 0) continue;
        uint8_t allowed = constraint_values[c_off + xi * dj_len + y];
        if (allowed) { supported = true; break; }
    }
    if (!supported) {
        domain_active[idx_i] = 0;
        *changed = 1;
    }
}

__global__ void forward_check_kernel(
    int var_count,
    const int* domain_offsets,
    int* domain_active,
    const int* assignment, // -1 if unassigned, else assigned index within domain
    int arc_count,
    const int* arc_i,
    const int* arc_j,
    const int* arc_dom_i_len,
    const int* arc_dom_j_len,
    const int* arc_constraint_offset,
    const uint8_t* constraint_values,
    int* changed
) {
    int var_j = blockIdx.x;
    int y = threadIdx.x;
    if (var_j >= var_count) return;
    int dj_len = domain_offsets[var_j + 1] - domain_offsets[var_j];
    if (y >= dj_len) return;
    int idx_j = domain_offsets[var_j] + y;
    if (domain_active[idx_j] == 0) return;

    // If any assigned neighbor disallows (x,y), prune y.
    for (int a = 0; a < arc_count; ++a) {
        if (arc_j[a] != var_j) continue;
        int var_i = arc_i[a];
        int xi = assignment[var_i];
        if (xi < 0) continue;
        int dj = arc_dom_j_len[a];
        int di = arc_dom_i_len[a];
        if (xi >= di || y >= dj) continue;
        int c_off = arc_constraint_offset[a];
        uint8_t allowed = constraint_values[c_off + xi * dj + y];
        if (!allowed) {
            domain_active[idx_j] = 0;
            *changed = 1;
            return;
        }
    }
}

extern "C" int gpu_ac3_revise(
    int arc_count,
    int var_count,
    const int* arc_i,
    const int* arc_j,
    const int* arc_dom_i_len,
    const int* arc_dom_j_len,
    const int* arc_constraint_offset,
    const int* domain_offsets,
    int* domain_active,
    const uint8_t* constraint_values
) {
    if (arc_count <= 0 || var_count <= 0) return 0;
    int domain_total = domain_offsets[var_count];
    // compute constraint value length
    int max_off = 0;
    for (int a = 0; a < arc_count; ++a) {
        int di = arc_dom_i_len[a];
        int dj = arc_dom_j_len[a];
        int off = arc_constraint_offset[a];
        int end = off + di * dj;
        if (end > max_off) max_off = end;
    }

    int *d_arc_i, *d_arc_j, *d_arc_di, *d_arc_dj, *d_arc_off;
    int *d_domain_offsets, *d_domain_active, *d_changed;
    uint8_t* d_constraints;
    cudaMalloc(&d_arc_i, sizeof(int) * arc_count);
    cudaMalloc(&d_arc_j, sizeof(int) * arc_count);
    cudaMalloc(&d_arc_di, sizeof(int) * arc_count);
    cudaMalloc(&d_arc_dj, sizeof(int) * arc_count);
    cudaMalloc(&d_arc_off, sizeof(int) * arc_count);
    cudaMalloc(&d_domain_offsets, sizeof(int) * (var_count + 1));
    cudaMalloc(&d_domain_active, sizeof(int) * domain_total);
    cudaMalloc(&d_constraints, sizeof(uint8_t) * max_off);
    cudaMalloc(&d_changed, sizeof(int));

    cudaMemcpy(d_arc_i, arc_i, sizeof(int) * arc_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_arc_j, arc_j, sizeof(int) * arc_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_arc_di, arc_dom_i_len, sizeof(int) * arc_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_arc_dj, arc_dom_j_len, sizeof(int) * arc_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_arc_off, arc_constraint_offset, sizeof(int) * arc_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_domain_offsets, domain_offsets, sizeof(int) * (var_count + 1), cudaMemcpyHostToDevice);
    cudaMemcpy(d_domain_active, domain_active, sizeof(int) * domain_total, cudaMemcpyHostToDevice);
    cudaMemcpy(d_constraints, constraint_values, sizeof(uint8_t) * max_off, cudaMemcpyHostToDevice);

    int h_changed = 0;
    cudaMemcpy(d_changed, &h_changed, sizeof(int), cudaMemcpyHostToDevice);
    ac3_revise_kernel<<<arc_count, 256>>>(
        arc_count, d_arc_i, d_arc_j, d_arc_di, d_arc_dj, d_arc_off,
        d_domain_offsets, d_domain_active, d_constraints, d_changed
    );
    cudaMemcpy(&h_changed, d_changed, sizeof(int), cudaMemcpyDeviceToHost);
    cudaMemcpy(domain_active, d_domain_active, sizeof(int) * domain_total, cudaMemcpyDeviceToHost);

    cudaFree(d_arc_i); cudaFree(d_arc_j); cudaFree(d_arc_di); cudaFree(d_arc_dj); cudaFree(d_arc_off);
    cudaFree(d_domain_offsets); cudaFree(d_domain_active); cudaFree(d_constraints); cudaFree(d_changed);
    return h_changed;
}

extern "C" int gpu_forward_check(
    int var_count,
    const int* domain_offsets,
    int* domain_active,
    const int* assignment,
    int arc_count,
    const int* arc_i,
    const int* arc_j,
    const int* arc_dom_i_len,
    const int* arc_dom_j_len,
    const int* arc_constraint_offset,
    const uint8_t* constraint_values
) {
    if (var_count <= 0) return 0;
    int domain_total = domain_offsets[var_count];
    int max_off = 0;
    for (int a = 0; a < arc_count; ++a) {
        int di = arc_dom_i_len[a];
        int dj = arc_dom_j_len[a];
        int off = arc_constraint_offset[a];
        int end = off + di * dj;
        if (end > max_off) max_off = end;
    }

    int *d_domain_offsets, *d_domain_active, *d_assignment, *d_arc_i, *d_arc_j, *d_arc_di, *d_arc_dj, *d_arc_off, *d_changed;
    uint8_t* d_constraints;
    cudaMalloc(&d_domain_offsets, sizeof(int) * (var_count + 1));
    cudaMalloc(&d_domain_active, sizeof(int) * domain_total);
    cudaMalloc(&d_assignment, sizeof(int) * var_count);
    cudaMalloc(&d_arc_i, sizeof(int) * arc_count);
    cudaMalloc(&d_arc_j, sizeof(int) * arc_count);
    cudaMalloc(&d_arc_di, sizeof(int) * arc_count);
    cudaMalloc(&d_arc_dj, sizeof(int) * arc_count);
    cudaMalloc(&d_arc_off, sizeof(int) * arc_count);
    cudaMalloc(&d_constraints, sizeof(uint8_t) * max_off);
    cudaMalloc(&d_changed, sizeof(int));

    cudaMemcpy(d_domain_offsets, domain_offsets, sizeof(int) * (var_count + 1), cudaMemcpyHostToDevice);
    cudaMemcpy(d_domain_active, domain_active, sizeof(int) * domain_total, cudaMemcpyHostToDevice);
    cudaMemcpy(d_assignment, assignment, sizeof(int) * var_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_arc_i, arc_i, sizeof(int) * arc_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_arc_j, arc_j, sizeof(int) * arc_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_arc_di, arc_dom_i_len, sizeof(int) * arc_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_arc_dj, arc_dom_j_len, sizeof(int) * arc_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_arc_off, arc_constraint_offset, sizeof(int) * arc_count, cudaMemcpyHostToDevice);
    cudaMemcpy(d_constraints, constraint_values, sizeof(uint8_t) * max_off, cudaMemcpyHostToDevice);

    int h_changed = 0;
    cudaMemcpy(d_changed, &h_changed, sizeof(int), cudaMemcpyHostToDevice);
    forward_check_kernel<<<var_count, 256>>>(
        var_count, d_domain_offsets, d_domain_active, d_assignment,
        arc_count, d_arc_i, d_arc_j, d_arc_di, d_arc_dj, d_arc_off,
        d_constraints, d_changed
    );
    cudaMemcpy(&h_changed, d_changed, sizeof(int), cudaMemcpyDeviceToHost);
    cudaMemcpy(domain_active, d_domain_active, sizeof(int) * domain_total, cudaMemcpyDeviceToHost);

    cudaFree(d_domain_offsets); cudaFree(d_domain_active); cudaFree(d_assignment);
    cudaFree(d_arc_i); cudaFree(d_arc_j); cudaFree(d_arc_di); cudaFree(d_arc_dj); cudaFree(d_arc_off);
    cudaFree(d_constraints); cudaFree(d_changed);
    return h_changed;
}

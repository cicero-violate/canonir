#include <cuda_runtime.h>
#include <stdint.h>

struct FeatureStats {
    int root_count;
    int leaf_count;
    int blocked_count;
    int ready_count;
    int failed_count;
    int completed_count;
    int verify_count;
    int mutate_count;
    int observe_count;
    int analysis_count;
    int render_count;
    int non_leaf_count;
    unsigned long long priority_sum;
    unsigned long long budget_sum;
    unsigned long long retry_sum;
    unsigned long long outdegree_sum;
};

__global__ void feature_kernel(
    const uint8_t* status,
    const int* indegree,
    const int* outdegree,
    const uint16_t* priority,
    const uint32_t* budget,
    const uint32_t* retry,
    const uint8_t* has_verify,
    const uint8_t* has_mutate,
    const uint8_t* has_observe,
    const uint8_t* node_type,
    int v,
    FeatureStats* out
) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= v) return;

    if (indegree[i] == 0) atomicAdd(&out->root_count, 1);
    if (outdegree[i] == 0) atomicAdd(&out->leaf_count, 1);
    if (outdegree[i] > 0) atomicAdd(&out->non_leaf_count, 1);
    atomicAdd(&out->outdegree_sum, (unsigned long long)outdegree[i]);

    uint8_t s = status[i];
    if (s == 5) atomicAdd(&out->blocked_count, 1);
    else if (s == 1) atomicAdd(&out->ready_count, 1);
    else if (s == 4) atomicAdd(&out->failed_count, 1);
    else if (s == 3) atomicAdd(&out->completed_count, 1);

    if (has_verify[i]) atomicAdd(&out->verify_count, 1);
    if (has_mutate[i]) atomicAdd(&out->mutate_count, 1);
    if (has_observe[i]) atomicAdd(&out->observe_count, 1);

    if (node_type[i] == 0) atomicAdd(&out->analysis_count, 1);
    else atomicAdd(&out->render_count, 1);

    atomicAdd(&out->priority_sum, (unsigned long long)priority[i]);
    atomicAdd(&out->budget_sum, (unsigned long long)budget[i]);
    atomicAdd(&out->retry_sum, (unsigned long long)retry[i]);
}

extern "C" void gpu_feature_stats(
    const uint8_t* status,
    const int* indegree,
    const int* outdegree,
    const uint16_t* priority,
    const uint32_t* budget,
    const uint32_t* retry,
    const uint8_t* has_verify,
    const uint8_t* has_mutate,
    const uint8_t* has_observe,
    const uint8_t* node_type,
    int v,
    FeatureStats* out
) {
    FeatureStats* d_out = nullptr;
    uint8_t *d_status = nullptr, *d_has_verify = nullptr, *d_has_mutate = nullptr, *d_has_observe = nullptr, *d_node_type = nullptr;
    int *d_indegree = nullptr, *d_outdegree = nullptr;
    uint16_t* d_priority = nullptr;
    uint32_t *d_budget = nullptr, *d_retry = nullptr;

    cudaMalloc(&d_out, sizeof(FeatureStats));
    cudaMemset(d_out, 0, sizeof(FeatureStats));
    cudaMalloc(&d_status, v * sizeof(uint8_t));
    cudaMalloc(&d_indegree, v * sizeof(int));
    cudaMalloc(&d_outdegree, v * sizeof(int));
    cudaMalloc(&d_priority, v * sizeof(uint16_t));
    cudaMalloc(&d_budget, v * sizeof(uint32_t));
    cudaMalloc(&d_retry, v * sizeof(uint32_t));
    cudaMalloc(&d_has_verify, v * sizeof(uint8_t));
    cudaMalloc(&d_has_mutate, v * sizeof(uint8_t));
    cudaMalloc(&d_has_observe, v * sizeof(uint8_t));
    cudaMalloc(&d_node_type, v * sizeof(uint8_t));

    cudaMemcpy(d_status, status, v * sizeof(uint8_t), cudaMemcpyHostToDevice);
    cudaMemcpy(d_indegree, indegree, v * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemcpy(d_outdegree, outdegree, v * sizeof(int), cudaMemcpyHostToDevice);
    cudaMemcpy(d_priority, priority, v * sizeof(uint16_t), cudaMemcpyHostToDevice);
    cudaMemcpy(d_budget, budget, v * sizeof(uint32_t), cudaMemcpyHostToDevice);
    cudaMemcpy(d_retry, retry, v * sizeof(uint32_t), cudaMemcpyHostToDevice);
    cudaMemcpy(d_has_verify, has_verify, v * sizeof(uint8_t), cudaMemcpyHostToDevice);
    cudaMemcpy(d_has_mutate, has_mutate, v * sizeof(uint8_t), cudaMemcpyHostToDevice);
    cudaMemcpy(d_has_observe, has_observe, v * sizeof(uint8_t), cudaMemcpyHostToDevice);
    cudaMemcpy(d_node_type, node_type, v * sizeof(uint8_t), cudaMemcpyHostToDevice);

    int threads = 256;
    int blocks = (v + threads - 1) / threads;
    feature_kernel<<<blocks, threads>>>(
        d_status,
        d_indegree,
        d_outdegree,
        d_priority,
        d_budget,
        d_retry,
        d_has_verify,
        d_has_mutate,
        d_has_observe,
        d_node_type,
        v,
        d_out
    );
    cudaDeviceSynchronize();

    cudaMemcpy(out, d_out, sizeof(FeatureStats), cudaMemcpyDeviceToHost);

    cudaFree(d_out);
    cudaFree(d_status);
    cudaFree(d_indegree);
    cudaFree(d_outdegree);
    cudaFree(d_priority);
    cudaFree(d_budget);
    cudaFree(d_retry);
    cudaFree(d_has_verify);
    cudaFree(d_has_mutate);
    cudaFree(d_has_observe);
    cudaFree(d_node_type);
}

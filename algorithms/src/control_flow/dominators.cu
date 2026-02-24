#include <cuda_runtime.h>
#include <stdint.h>

// Variables:
//   N        = number of nodes
//   pred_row = CSR row ptr for predecessor lists (len N+1)
//   pred_col = CSR col idx for predecessors (len E)
//   words    = number of u64 words per bitset (ceil(N/64))
//   dom      = N x words bitsets (output)
//
// Equation:
//   dom(entry) = {entry}
//   dom(n)     = {n} ∪ ⋂_{p in pred(n)} dom(p)

__global__ void dom_step(
    const int* pred_row, const int* pred_col,
    const unsigned long long* dom_in,
    unsigned long long* dom_out,
    int N, int words, int entry, int* changed)
{
    int n = blockIdx.x * blockDim.x + threadIdx.x;
    if (n >= N) return;

    unsigned long long* out = dom_out + (size_t)n * words;
    const unsigned long long* in = dom_in + (size_t)n * words;

    if (n == entry) {
        for (int w = 0; w < words; w++) {
            out[w] = in[w];
        }
        return;
    }

    int start = pred_row[n];
    int end   = pred_row[n + 1];

    // If no preds, start from all-ones (same as CPU algorithm).
    for (int w = 0; w < words; w++) {
        out[w] = ~0ULL;
    }

    for (int ei = start; ei < end; ei++) {
        int p = pred_col[ei];
        const unsigned long long* pd = dom_in + (size_t)p * words;
        for (int w = 0; w < words; w++) {
            out[w] &= pd[w];
        }
    }

    // Add self bit.
    int word = n >> 6;
    int bit  = n & 63;
    out[word] |= (1ULL << bit);

    // Change detection.
    for (int w = 0; w < words; w++) {
        if (out[w] != in[w]) { *changed = 1; break; }
    }
}

extern "C" void gpu_dominators(
    const int* pred_row, const int* pred_col,
    int N, int entry, int words,
    unsigned long long* dom_out)
{
    int* d_row = nullptr;
    int* d_col = nullptr;
    unsigned long long* d_dom_a = nullptr;
    unsigned long long* d_dom_b = nullptr;
    int* d_changed = nullptr;

    size_t row_bytes = (size_t)(N + 1) * sizeof(int);
    size_t col_bytes = (size_t)pred_row[N] * sizeof(int);
    size_t dom_bytes = (size_t)N * (size_t)words * sizeof(unsigned long long);

    cudaMalloc(&d_row, row_bytes);
    cudaMalloc(&d_col, col_bytes);
    cudaMalloc(&d_dom_a, dom_bytes);
    cudaMalloc(&d_dom_b, dom_bytes);
    cudaMalloc(&d_changed, sizeof(int));

    cudaMemcpy(d_row, pred_row, row_bytes, cudaMemcpyHostToDevice);
    cudaMemcpy(d_col, pred_col, col_bytes, cudaMemcpyHostToDevice);

    // Init dom on host: all ones, except entry.
    unsigned long long* h_dom = (unsigned long long*)malloc(dom_bytes);
    for (int n = 0; n < N; n++) {
        for (int w = 0; w < words; w++) {
            h_dom[(size_t)n * words + w] = ~0ULL;
        }
    }
    for (int w = 0; w < words; w++) {
        h_dom[(size_t)entry * words + w] = 0ULL;
    }
    h_dom[(size_t)entry * words + (entry >> 6)] = (1ULL << (entry & 63));

    cudaMemcpy(d_dom_a, h_dom, dom_bytes, cudaMemcpyHostToDevice);
    free(h_dom);

    int threads = 256;
    int blocks = (N + threads - 1) / threads;

    for (int iter = 0; iter < N; iter++) {
        cudaMemset(d_changed, 0, sizeof(int));
        dom_step<<<blocks, threads>>>(
            d_row, d_col, d_dom_a, d_dom_b, N, words, entry, d_changed
        );
        int h_changed = 0;
        cudaMemcpy(&h_changed, d_changed, sizeof(int), cudaMemcpyDeviceToHost);
        if (!h_changed) {
            break;
        }
        unsigned long long* tmp = d_dom_a;
        d_dom_a = d_dom_b;
        d_dom_b = tmp;
    }

    cudaMemcpy(dom_out, d_dom_a, dom_bytes, cudaMemcpyDeviceToHost);
    cudaFree(d_row);
    cudaFree(d_col);
    cudaFree(d_dom_a);
    cudaFree(d_dom_b);
    cudaFree(d_changed);
}

#include <cuda_runtime.h>
#include <stdint.h>

// Variables:
//   B        = number of blocks
//   pred_row = CSR row ptr for predecessors (len B+1)
//   pred_col = CSR col idx for predecessors (len E)
//   words    = number of u64 words per bitset (ceil(D/64))
//   gen      = B x words
//   kill     = B x words
//   out      = B x words (output)
//
// Equations:
//   in(b)  = OR_{p in pred(b)} out(p)
//   out(b) = gen(b) OR (in(b) & ~kill(b))

__global__ void dataflow_step(
    const int* pred_row, const int* pred_col,
    const unsigned long long* gen,
    const unsigned long long* kill,
    const unsigned long long* out_in,
    unsigned long long* out_out,
    int B, int words, int* changed)
{
    int b = blockIdx.x * blockDim.x + threadIdx.x;
    if (b >= B) return;

    const unsigned long long* gen_b  = gen + (size_t)b * words;
    const unsigned long long* kill_b = kill + (size_t)b * words;
    const unsigned long long* out_b  = out_in + (size_t)b * words;
    unsigned long long* out_new      = out_out + (size_t)b * words;

    int start = pred_row[b];
    int end   = pred_row[b + 1];

    // Compute in(b) as OR over predecessors.
    for (int w = 0; w < words; w++) {
        unsigned long long inw = 0ULL;
        for (int ei = start; ei < end; ei++) {
            int p = pred_col[ei];
            inw |= out_in[(size_t)p * words + w];
        }
        out_new[w] = gen_b[w] | (inw & ~kill_b[w]);
    }

    for (int w = 0; w < words; w++) {
        if (out_new[w] != out_b[w]) { *changed = 1; break; }
    }
}

extern "C" void gpu_reaching_definitions(
    const int* pred_row, const int* pred_col,
    int B, int words,
    const unsigned long long* gen,
    const unsigned long long* kill,
    unsigned long long* out)
{
    int* d_row = nullptr;
    int* d_col = nullptr;
    unsigned long long* d_gen = nullptr;
    unsigned long long* d_kill = nullptr;
    unsigned long long* d_out_a = nullptr;
    unsigned long long* d_out_b = nullptr;
    int* d_changed = nullptr;

    size_t row_bytes = (size_t)(B + 1) * sizeof(int);
    size_t col_bytes = (size_t)pred_row[B] * sizeof(int);
    size_t mat_bytes = (size_t)B * (size_t)words * sizeof(unsigned long long);

    cudaMalloc(&d_row, row_bytes);
    cudaMalloc(&d_col, col_bytes);
    cudaMalloc(&d_gen, mat_bytes);
    cudaMalloc(&d_kill, mat_bytes);
    cudaMalloc(&d_out_a, mat_bytes);
    cudaMalloc(&d_out_b, mat_bytes);
    cudaMalloc(&d_changed, sizeof(int));

    cudaMemcpy(d_row, pred_row, row_bytes, cudaMemcpyHostToDevice);
    cudaMemcpy(d_col, pred_col, col_bytes, cudaMemcpyHostToDevice);
    cudaMemcpy(d_gen, gen, mat_bytes, cudaMemcpyHostToDevice);
    cudaMemcpy(d_kill, kill, mat_bytes, cudaMemcpyHostToDevice);
    cudaMemcpy(d_out_a, gen, mat_bytes, cudaMemcpyHostToDevice);

    int threads = 256;
    int blocks = (B + threads - 1) / threads;

    for (int iter = 0; iter < B; iter++) {
        cudaMemset(d_changed, 0, sizeof(int));
        dataflow_step<<<blocks, threads>>>(
            d_row, d_col, d_gen, d_kill, d_out_a, d_out_b, B, words, d_changed
        );
        int h_changed = 0;
        cudaMemcpy(&h_changed, d_changed, sizeof(int), cudaMemcpyDeviceToHost);
        if (!h_changed) {
            break;
        }
        unsigned long long* tmp = d_out_a;
        d_out_a = d_out_b;
        d_out_b = tmp;
    }

    cudaMemcpy(out, d_out_a, mat_bytes, cudaMemcpyDeviceToHost);
    cudaFree(d_row);
    cudaFree(d_col);
    cudaFree(d_gen);
    cudaFree(d_kill);
    cudaFree(d_out_a);
    cudaFree(d_out_b);
    cudaFree(d_changed);
}

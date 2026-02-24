#include <cuda_runtime.h>
#include <curand_kernel.h>
#include <stdint.h>

// Variables:
//   POP  = population size
//   GEN  = number of generations
//   tid  = thread id, each thread owns one individual
//   seed = base RNG seed, varied per thread and generation
//
// Equation (parallel tournament selection + arithmetic crossover):
//   fitness(x) = x   (identity; caller can substitute)
//   parent_a   = max(pop[ia], pop[ib])   where ia,ib ~ Uniform(0,POP)
//   parent_b   = max(pop[ic], pop[id])   where ic,id ~ Uniform(0,POP)
//   child      = (parent_a + parent_b) / 2
//   result     = max_i population[i]  after GEN rounds

__global__ void evolve_kernel(
    uint64_t* pop,
    int POP,
    unsigned long long seed,
    int gen)
{
    int tid = blockIdx.x * blockDim.x + threadIdx.x;
    if (tid >= POP) return;

    curandState s;
    curand_init(seed + tid + (unsigned long long)gen * POP,
                0, 0, &s);

    uint64_t a = pop[curand(&s) % POP];
    uint64_t b = pop[curand(&s) % POP];
    uint64_t c = pop[curand(&s) % POP];
    uint64_t d = pop[curand(&s) % POP];

    uint64_t parent_a = (a > b) ? a : b;
    uint64_t parent_b = (c > d) ? c : d;

    pop[tid] = (parent_a + parent_b) / 2;
}

extern "C" uint64_t gpu_genetic_optimize(
    uint64_t* population,
    int POP,
    int GEN)
{
    uint64_t* d;
    size_t bytes = (size_t)POP * sizeof(uint64_t);

    cudaMalloc(&d, bytes);
    cudaMemcpy(d, population, bytes, cudaMemcpyHostToDevice);

    int threads = 256;
    int blocks  = (POP + threads - 1) / threads;

    for (int g = 0; g < GEN; g++) {
        evolve_kernel<<<blocks, threads>>>(d, POP, 42ULL, g);
        cudaDeviceSynchronize();
    }

    cudaMemcpy(population, d, bytes, cudaMemcpyDeviceToHost);
    cudaFree(d);

    uint64_t best = 0;
    for (int i = 0; i < POP; i++) {
        if (population[i] > best) {
            best = population[i];
        }
    }

    return best;
}

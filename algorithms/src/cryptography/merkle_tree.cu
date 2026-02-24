#include <cuda_runtime.h>
#include <stdint.h>

// Variables:
//   L          = leaf count (power of 2)
//   PAGE_SIZE  = bytes per leaf block
//   HASH_SIZE  = 32 (SHA-256 output bytes)
//   tree[]     = flat 2*L node array, each 32 bytes, 1-indexed
//                tree[L+i] = SHA256(page_i)             (leaves)
//                tree[j]   = SHA256(tree[2j] || tree[2j+1]) (parents)
//
// Equations:
//   Leaf pass:   one thread per page  -> SHA256(4096 bytes) -> tree[L+tid]
//   Parent pass: one thread per pair  -> SHA256(32||32)     -> tree[j]
//   log2(L) parent passes reduce tree to root at tree[1]
//
//   SHA-256 compress: state' = compress(state, block)
//   H(left||right): single 64-byte block, no length padding needed here
//                   (simplified for fixed 64-byte input)

#define HASH_SIZE  32
#define BLOCK_SIZE 64
#define PAGE_SIZE  4096

__device__ __constant__ uint32_t K[64] = {
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,
    0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,
    0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,
    0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,
    0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,
    0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,
    0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,
    0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,
    0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2
};

__device__ __forceinline__ uint32_t rotr(uint32_t x, uint32_t n) {
    return (x >> n) | (x << (32 - n));
}

// Single-block SHA-256: exactly 64 bytes input (left||right hash pair).
__device__ void sha256_internal(
    const uint8_t* left, const uint8_t* right, uint8_t* out)
{
    uint32_t w[64];
    uint8_t  block[64];
    #pragma unroll
    for (int i = 0; i < 32; i++) block[i]      = left[i];
    #pragma unroll
    for (int i = 0; i < 32; i++) block[i + 32] = right[i];
    #pragma unroll
    for (int i = 0; i < 16; i++)
        w[i] = (block[i*4]<<24)|(block[i*4+1]<<16)|(block[i*4+2]<<8)|block[i*4+3];
    #pragma unroll
    for (int i = 16; i < 64; i++) {
        uint32_t s0 = rotr(w[i-15],7)^rotr(w[i-15],18)^(w[i-15]>>3);
        uint32_t s1 = rotr(w[i-2],17)^rotr(w[i-2],19)^(w[i-2]>>10);
        w[i] = w[i-16]+s0+w[i-7]+s1;
    }
    uint32_t a=0x6a09e667,b=0xbb67ae85,c=0x3c6ef372,d=0xa54ff53a;
    uint32_t e=0x510e527f,f=0x9b05688c,g=0x1f83d9ab,h=0x5be0cd19;
    #pragma unroll
    for (int i = 0; i < 64; i++) {
        uint32_t S1=rotr(e,6)^rotr(e,11)^rotr(e,25);
        uint32_t ch=(e&f)^(~e&g);
        uint32_t t1=h+S1+ch+K[i]+w[i];
        uint32_t S0=rotr(a,2)^rotr(a,13)^rotr(a,22);
        uint32_t maj=(a&b)^(a&c)^(b&c);
        uint32_t t2=S0+maj;
        h=g;g=f;f=e;e=d+t1;d=c;c=b;b=a;a=t1+t2;
    }
    uint32_t st[8]={a+0x6a09e667,b+0xbb67ae85,c+0x3c6ef372,d+0xa54ff53a,
                    e+0x510e527f,f+0x9b05688c,g+0x1f83d9ab,h+0x5be0cd19};
    #pragma unroll
    for (int i=0;i<8;i++){
        out[i*4]=(st[i]>>24)&0xff; out[i*4+1]=(st[i]>>16)&0xff;
        out[i*4+2]=(st[i]>>8)&0xff; out[i*4+3]=st[i]&0xff;
    }
}

// Multi-block SHA-256 for PAGE_SIZE bytes.
__device__ void sha256_page(const uint8_t* data, uint8_t* out) {
    uint32_t h[8]={0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,
                   0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19};
    uint32_t w[64];
    for (int chunk=0; chunk<PAGE_SIZE; chunk+=64) {
        #pragma unroll
        for (int i=0;i<16;i++) {
            int b=chunk+i*4;
            w[i]=(data[b]<<24)|(data[b+1]<<16)|(data[b+2]<<8)|data[b+3];
        }
        #pragma unroll
        for (int i=16;i<64;i++){
            uint32_t s0=rotr(w[i-15],7)^rotr(w[i-15],18)^(w[i-15]>>3);
            uint32_t s1=rotr(w[i-2],17)^rotr(w[i-2],19)^(w[i-2]>>10);
            w[i]=w[i-16]+s0+w[i-7]+s1;
        }
        uint32_t a=h[0],b=h[1],c=h[2],d=h[3],e=h[4],f=h[5],g=h[6],hh=h[7];
        #pragma unroll
        for (int i=0;i<64;i++){
            uint32_t S1=rotr(e,6)^rotr(e,11)^rotr(e,25);
            uint32_t ch=(e&f)^(~e&g);
            uint32_t t1=hh+S1+ch+K[i]+w[i];
            uint32_t S0=rotr(a,2)^rotr(a,13)^rotr(a,22);
            uint32_t maj=(a&b)^(a&c)^(b&c);
            uint32_t t2=S0+maj;
            hh=g;g=f;f=e;e=d+t1;d=c;c=b;b=a;a=t1+t2;
        }
        h[0]+=a;h[1]+=b;h[2]+=c;h[3]+=d;
        h[4]+=e;h[5]+=f;h[6]+=g;h[7]+=hh;
    }
    #pragma unroll
    for (int i=0;i<8;i++){
        out[i*4]=(h[i]>>24)&0xff; out[i*4+1]=(h[i]>>16)&0xff;
        out[i*4+2]=(h[i]>>8)&0xff; out[i*4+3]=h[i]&0xff;
    }
}

extern "C" __global__ void hash_leaves_kernel(
    uint8_t* tree, uint64_t leaf_count, const uint8_t* pages)
{
    uint64_t tid = blockIdx.x * blockDim.x + threadIdx.x;
    if (tid >= leaf_count) return;
    sha256_page(&pages[tid * PAGE_SIZE], &tree[(leaf_count + tid) * HASH_SIZE]);
}

extern "C" __global__ void hash_parents_kernel(
    uint8_t* tree, uint64_t level_offset, uint64_t parent_count)
{
    uint64_t tid = blockIdx.x * blockDim.x + threadIdx.x;
    if (tid >= parent_count) return;
    uint64_t left  = level_offset + tid * 2;
    uint64_t right = left + 1;
    uint64_t par   = (level_offset / 2) + tid;
    sha256_internal(&tree[left*HASH_SIZE], &tree[right*HASH_SIZE], &tree[par*HASH_SIZE]);
}

// tree_host: host buffer, length 2*L*HASH_SIZE (1-indexed, node 0 unused)
// pages_host: host buffer, length L*PAGE_SIZE
extern "C" void gpu_merkle_build(
    uint8_t*       tree_host,
    uint64_t       leaf_count,
    const uint8_t* pages_host)
{
    if (leaf_count == 0) return;
    const int threads = 256;

    size_t tree_bytes  = 2ULL * leaf_count * HASH_SIZE;
    size_t pages_bytes = leaf_count * (uint64_t)PAGE_SIZE;

    uint8_t *d_tree, *d_pages;
    cudaMalloc(&d_tree,  tree_bytes);
    cudaMalloc(&d_pages, pages_bytes);
    cudaMemset(d_tree, 0, tree_bytes);
    cudaMemcpy(d_pages, pages_host, pages_bytes, cudaMemcpyHostToDevice);

    cudaStream_t s_leaf, s_reduce;
    cudaEvent_t  leaves_done;
    cudaStreamCreate(&s_leaf);
    cudaStreamCreate(&s_reduce);
    cudaEventCreate(&leaves_done);

    // Phase 1: hash leaves in parallel — one thread per page
    int lb = (leaf_count + threads - 1) / threads;
    hash_leaves_kernel<<<lb, threads, 0, s_leaf>>>(d_tree, leaf_count, d_pages);
    cudaEventRecord(leaves_done, s_leaf);
    cudaStreamWaitEvent(s_reduce, leaves_done, 0);

    // Phase 2: bottom-up parent reduction
    uint64_t level_size   = leaf_count;
    uint64_t level_offset = leaf_count;
    while (level_size > 1) {
        uint64_t parents = level_size / 2;
        int pb = (parents + threads - 1) / threads;
        hash_parents_kernel<<<pb, threads, 0, s_reduce>>>(d_tree, level_offset, parents);
        level_offset /= 2;
        level_size   /= 2;
    }

    cudaStreamSynchronize(s_reduce);
    cudaEventDestroy(leaves_done);
    cudaStreamDestroy(s_leaf);
    cudaStreamDestroy(s_reduce);

    cudaMemcpy(tree_host, d_tree, tree_bytes, cudaMemcpyDeviceToHost);
    cudaFree(d_tree);
    cudaFree(d_pages);
}

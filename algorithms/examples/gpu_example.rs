//! Demonstrates all GPU algorithm wrappers (requires --features cuda).
//!
//! Run with:
//!   cargo run --example gpu_example --features cuda

#[cfg(feature = "cuda")]
use algorithms::control_flow::gpu::{dominators_gpu, reaching_definitions_gpu};
#[cfg(feature = "cuda")]
use algorithms::cryptography::merkle_tree_gpu::{merkle_build_gpu, root, PAGE_SIZE};
#[cfg(feature = "cuda")]
use algorithms::graph::bellman_ford_gpu::bellman_ford_gpu;
#[cfg(feature = "cuda")]
use algorithms::graph::csr_unified::CsrUnified;
#[cfg(feature = "cuda")]
use algorithms::graph::gpu::bfs_gpu as bfs_gpu_csr;
#[cfg(feature = "cuda")]
use algorithms::graph::{adj_list::AdjList, gpu::bfs_gpu};
#[cfg(feature = "cuda")]
use algorithms::numerical::gpu::{matrix_multiply_gpu, sieve_gpu};
#[cfg(feature = "cuda")]
use algorithms::optimization::gpu::genetic_optimize_gpu;
#[cfg(feature = "cuda")]
use algorithms::searching::gpu::linear_search_gpu;
#[cfg(feature = "cuda")]
use algorithms::sorting::gpu::bitonic_sort_gpu;
#[cfg(feature = "cuda")]
use algorithms::string_algorithms::gpu::rabin_karp_gpu;

fn main() {
    #[cfg(not(feature = "cuda"))]
    {
        eprintln!("Rebuild with --features cuda to run GPU examples.");
        return;
    }

    #[cfg(feature = "cuda")]
    {
        let stress = std::env::args().any(|a| a == "--stress");

        // ── 1. Graph BFS ─────────────────────────────────────────────────────
        // Graph:  0->1, 0->2, 1->3, 2->3
        // BFS levels from source 0: [0, 1, 1, 2]
        println!("=== GPU BFS ===");
        let (v, chain) = if stress { (1 << 20, true) } else { (4, false) };
        let mut g = AdjList::new(v);
        if chain {
            // linear chain: 0->1->2->...->v-1, stays busy on GPU
            for i in 0..v - 1 {
                g.add_edge(i, i + 1);
            }
            println!("(stress) {} vertices, linear chain", v);
        } else {
            g.add_edge(0, 1);
            g.add_edge(0, 2);
            g.add_edge(1, 3);
            g.add_edge(2, 3);
        }
        let csr = g.to_csr();
        let levels = bfs_gpu(&csr, 0);

        // Same graph via unified memory CSR — no cudaMemcpy in the kernel
        let ucsr = CsrUnified::from_adj(&g);
        println!("row_ptr (unified, host-readable): {:?}", ucsr.row_ptr_slice());
        println!("col_idx (unified, host-readable): {:?}", ucsr.col_idx_slice());
        // Pass unified pointers directly to bfs kernel via Csr wrapper
        let csr2 = algorithms::graph::csr::Csr { row_ptr: ucsr.row_ptr_slice().to_vec(), col_idx: ucsr.col_idx_slice().to_vec() };
        let levels2 = bfs_gpu_csr(&csr2, 0);
        println!("BFS via unified CSR: {:?}", levels2);
        if stress {
            println!("max level: {}", levels.iter().max().unwrap());
        } else {
            println!("levels from source 0: {:?}", levels);
        }

        // ── 2. Bitonic sort ──────────────────────────────────────────────────
        // Pads to next power-of-2 internally, trims after.
        // O(log^2 N) passes, all comparisons parallel within each pass.
        println!("\n=== GPU Bitonic Sort ===");
        let mut arr: Vec<i64> = if stress {
            let n = 1 << 24; // 16M elements
            println!("(stress) sorting {} elements", n);
            (0..n as i64).rev().collect()
        } else {
            vec![9, 3, 7, 1, 5, 8, 2, 6, 4, 0]
        };
        if !stress {
            println!("before: {:?}", arr);
        }
        bitonic_sort_gpu(&mut arr);
        if stress {
            println!("sorted: first={} last={}", arr[0], arr[arr.len() - 1]);
        } else {
            println!("after:  {:?}", arr);
        }

        // ── 3. Linear search ─────────────────────────────────────────────────
        // N threads each check one element; atomicMin picks first match.
        println!("\n=== GPU Linear Search ===");
        let haystack: Vec<i64> = if stress {
            let n = 1 << 26; // 64M elements
            println!("(stress) searching {} elements", n);
            (0..n as i64).collect()
        } else {
            (0..20).collect()
        };
        let target = if stress { (1 << 25) as i64 } else { 13i64 };
        match linear_search_gpu(&haystack, target) {
            Some(idx) => println!("found {} at index {}", target, idx),
            None => println!("{} not found", target),
        }

        // ── 4. Matrix multiply ───────────────────────────────────────────────
        // C[r][c] = sum_k A[r][k] * B[k][c]
        // 3x3 identity x identity = identity
        println!("\n=== GPU Matrix Multiply (3x3) ===");
        #[rustfmt::skip]
        let identity: Vec<i64> = vec![
            1, 0, 0,
            0, 1, 0,
            0, 0, 1,
        ];
        let c = matrix_multiply_gpu(&identity, &identity, 3);
        for row in 0..3 {
            println!("  {:?}", &c[row * 3..row * 3 + 3]);
        }

        // ── 5. Dominators (GPU) ──────────────────────────────────────────────
        // CFG: 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3
        println!("\n=== GPU Dominators ===");
        let mut pred_adj: Vec<Vec<usize>> = vec![vec![], vec![0], vec![0], vec![1, 2]];
        let pred_csr = algorithms::graph::csr::Csr::from_adj(&pred_adj);
        let dom_bits = dominators_gpu(&pred_csr, 0, 4);
        let words = (4 + 63) / 64;
        for n in 0..4 {
            let mut doms = Vec::new();
            for i in 0..4 {
                let word = dom_bits[n * words + (i >> 6)];
                if (word >> (i & 63)) & 1 == 1 {
                    doms.push(i);
                }
            }
            println!("  dom({n}) = {:?}", doms);
        }

        // ── 6. Reaching Definitions (GPU) ────────────────────────────────────
        // Blocks: 0 -> 1 -> 2, defs: d0 in 0, d1 in 1
        println!("\n=== GPU Reaching Definitions ===");
        let pred_adj: Vec<Vec<usize>> = vec![vec![], vec![0], vec![1]];
        let pred_csr = algorithms::graph::csr::Csr::from_adj(&pred_adj);
        let block_count = 3;
        let def_count = 2;
        let words = (def_count + 63) / 64;
        let mut r#gen = vec![0u64; block_count * words];
        let mut kill = vec![0u64; block_count * words];
        r#gen[0] |= 1u64 << 0;
        r#gen[1] |= 1u64 << 1;
        let out_bits = reaching_definitions_gpu(&pred_csr, block_count, def_count, &r#gen, &kill);
        for b in 0..block_count {
            let mut defs = Vec::new();
            for i in 0..def_count {
                let word = out_bits[b * words + (i >> 6)];
                if (word >> (i & 63)) & 1 == 1 {
                    defs.push(i);
                }
            }
            println!("  out({b}) = {:?}", defs);
        }

        // ── 5. Sieve of Eratosthenes ─────────────────────────────────────────
        // Each kernel launch marks all multiples of one prime in parallel.
        println!("\n=== GPU Sieve (primes up to 50) ===");
        let limit = if stress { 100_000_000 } else { 50 };
        if stress {
            println!("(stress) sieve up to {}", limit);
        }
        let primes = sieve_gpu(limit);
        if stress {
            println!("prime count: {}  largest: {}", primes.len(), primes.last().unwrap());
        } else {
            println!("primes: {:?}", primes);
        }

        // ── 6. Rabin-Karp string search ──────────────────────────────────────
        // One thread per window; rolling hash match then char verify.
        println!("\n=== GPU Rabin-Karp ===");
        let text = b"abracadabra";
        let pattern = b"abra";
        let matches = rabin_karp_gpu(text, pattern);
        println!("pattern \"{}\" found at positions: {:?}", std::str::from_utf8(pattern).unwrap(), matches);

        // ── 7. Genetic algorithm ─────────────────────────────────────────────
        // Each thread: tournament select two parents, arithmetic crossover.
        // fitness = identity (maximise value), 20 generations.
        println!("\n=== GPU Genetic Algorithm ===");
        let mut population: Vec<u64> = vec![1, 50, 23, 8, 99, 42, 17, 65];
        let best = genetic_optimize_gpu(&mut population, 20);
        println!("best individual after 20 generations: {}", best);

        // ── 9. Merkle Tree ───────────────────────────────────────────────────
        // Phase 1: one thread per page -> SHA256(4096 bytes) -> leaf node
        // Phase 2: log2(L) reduction passes, one thread per parent node
        //          SHA256(left_child_hash || right_child_hash)
        // root = tree[32..64]  (1-indexed, node 1)
        println!("\n=== GPU Merkle Tree ===");
        let leaf_count = if stress { 1024 } else { 4 };
        let pages = vec![0xabu8; leaf_count * PAGE_SIZE];
        let tree = merkle_build_gpu(&pages);
        let r = root(&tree);
        println!("leaves: {}  root: {}", leaf_count, r.iter().map(|b| format!("{:02x}", b)).collect::<String>());

        // ── 8. Bellman-Ford ──────────────────────────────────────────────────
        // One thread per edge per pass; V-1 passes total.
        // Edges: (from, to, weight)
        // Graph: 0->1(4), 0->2(1), 2->1(2), 1->3(1)
        // dist from 0: [0, 3, 1, 4]
        println!("\n=== GPU Bellman-Ford ===");
        let edges: Vec<(usize, usize, u64)> = vec![(0, 1, 4), (0, 2, 1), (2, 1, 2), (1, 3, 1)];
        match bellman_ford_gpu(4, &edges, 0) {
            Ok(dist) => println!("dist from 0: {:?}", dist),
            Err(msg) => println!("error: {}", msg),
        }

        if stress {
            println!("\n(stress) Bellman-Ford on {} vertices {} edges", 1024, 1024 * 8);
            let mut stress_edges: Vec<(usize, usize, u64)> = Vec::new();
            for i in 0usize..1024 {
                for j in 1..=8usize {
                    stress_edges.push((i, (i + j) % 1024, j as u64));
                }
            }
            match bellman_ford_gpu(1024, &stress_edges, 0) {
                Ok(dist) => println!("max dist: {}", dist.iter().filter(|&&d| d < u64::MAX / 2).max().unwrap()),
                Err(msg) => println!("error: {}", msg),
            }
        }
    }
}

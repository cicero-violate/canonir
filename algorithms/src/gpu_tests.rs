//! Integration tests for all GPU algorithm wrappers.
//! Run with: cargo test --features cuda

#[cfg(all(test, feature = "cuda"))]
mod gpu_tests {
    use crate::graph::adj_list::AdjList;
    use crate::graph::csr::Csr;
    use crate::graph::gpu::bfs_gpu;
    use crate::graph::reachability::reachability_gpu;
    use crate::graph::max_flow::max_flow_gpu;
    use crate::constraints::ac3::{ConstraintGraph, ac3_gpu_apply};
    use crate::constraints::forward_checking::forward_check_gpu_build;
    use crate::sorting::gpu::bitonic_sort_gpu;
    use crate::searching::gpu::linear_search_gpu;
    use crate::numerical::gpu::{matrix_multiply_gpu, sieve_gpu};
    use crate::string_algorithms::gpu::rabin_karp_gpu;
    use crate::cryptography::merkle_tree_gpu::{merkle_build_gpu, root, PAGE_SIZE};
    use crate::graph::bellman_ford_gpu::bellman_ford_gpu;
    use crate::graph::csr_unified::CsrUnified;
    use crate::graph::model_checking::model_check_gpu;
    use crate::control_flow::gpu::{dominators_gpu, reaching_definitions_gpu};
    use crate::graph::scheduler_gpu::{ready_mask_gpu, pack_ready_priority, deadlock_gpu};
    use crate::graph::topological_sort_gpu::topological_sort_gpu;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn diamond_csr() -> Csr {
        // 0->1, 0->2, 1->3, 2->3
        let mut g = AdjList::new(4);
        g.add_edge(0, 1);
        g.add_edge(0, 2);
        g.add_edge(1, 3);
        g.add_edge(2, 3);
        g.to_csr()
    }

    #[test]
    fn topo_sort_gpu_respects_edges() {
        // 0->1, 0->2, 1->3, 2->3
        let csr = diamond_csr();
        let order = topological_sort_gpu(&csr);
        assert_eq!(order.len(), 4);
        let mut pos = vec![0usize; 4];
        for (i, &n) in order.iter().enumerate() {
            pos[n] = i;
        }
        assert!(pos[0] < pos[1]);
        assert!(pos[0] < pos[2]);
        assert!(pos[1] < pos[3]);
        assert!(pos[2] < pos[3]);
    }

    // ── Scheduler GPU ───────────────────────────────────────────────────────

    #[test]
    fn scheduler_ready_mask_simple_chain() {
        // 0 completed, 1 depends on 0, 2 depends on 1
        let status = vec![3u8, 0u8, 0u8]; // Completed, Pending, Pending
        let deps_offset = vec![0i32, 0, 1, 2];
        let deps_flat = vec![0i32, 1i32];
        let (ready, ready_count, completed) = ready_mask_gpu(&status, &deps_offset, &deps_flat);
        assert_eq!(ready, vec![0u8, 1u8, 0u8]);
        assert_eq!(ready_count, 1);
        assert_eq!(completed, 1);
    }

    #[test]
    fn scheduler_deadlock_cycle() {
        // cycle between 0 and 1, both pending => deadlock
        let status = vec![0u8, 0u8];
        let deps_offset = vec![0i32, 1, 2];
        let deps_flat = vec![1i32, 0i32];
        assert!(deadlock_gpu(&status, &deps_offset, &deps_flat));
    }

    #[test]
    fn scheduler_priority_sort_orders_highest_first() {
        let ready = vec![1u8, 1u8, 0u8];
        let priority = vec![2u16, 5u16, 0u16];
        let mut keys = pack_ready_priority(&ready, &priority);
        bitonic_sort_gpu(&mut keys);
        let mut order = Vec::new();
        for key in keys.into_iter().rev() {
            if key < 0 { continue; }
            order.push((key & 0xFFFF_FFFF) as usize);
        }
        assert_eq!(order, vec![1, 0]);
    }

    // ── BFS ──────────────────────────────────────────────────────────────────

    #[test]
    fn bfs_diamond_levels() {
        // L(0)=0, L(1)=1, L(2)=1, L(3)=2
        let csr = diamond_csr();
        let levels = bfs_gpu(&csr, 0);
        assert_eq!(levels, vec![0, 1, 1, 2]);
    }

    #[test]
    fn bfs_source_unreachable_marked_minus_one() {
        // isolated graph: 0->1, source=2 -> only node 2 is reachable
        let csr = Csr::from_edges(3, &[(0, 1)]);
        let levels = bfs_gpu(&csr, 2);
        assert_eq!(levels[0], -1);
        assert_eq!(levels[1], -1);
        assert_eq!(levels[2], 0);
    }

    #[test]
    fn bfs_linear_chain() {
        // 0->1->2->3->4, BFS from 0
        let csr = Csr::from_edges(5, &[(0,1),(1,2),(2,3),(3,4)]);
        let levels = bfs_gpu(&csr, 0);
        assert_eq!(levels, vec![0, 1, 2, 3, 4]);
    }

    // ── Reachability ─────────────────────────────────────────────────────────

    #[test]
    fn reachability_diamond_from_zero() {
        // from root 0, all 4 nodes reachable
        let csr = diamond_csr();
        let reached = reachability_gpu(&csr, &[0]);
        assert_eq!(reached, vec![true, true, true, true]);
    }

    #[test]
    fn reachability_multi_root() {
        // roots {0, 2}: all nodes reachable
        let csr = diamond_csr();
        let reached = reachability_gpu(&csr, &[0, 2]);
        assert!(reached.iter().all(|&v| v));
    }

    #[test]
    fn reachability_isolated_node_not_reached() {
        // 0->1, node 2 isolated; root=0 should not reach node 2
        let csr = Csr::from_edges(3, &[(0, 1)]);
        let reached = reachability_gpu(&csr, &[0]);
        assert_eq!(reached[0], true);
        assert_eq!(reached[1], true);
        assert_eq!(reached[2], false);
    }

    #[test]
    fn reachability_linear_chain_full() {
        // 0->1->2->...->99, root=0 should reach all
        let edges: Vec<(usize, usize)> = (0..99).map(|i| (i, i + 1)).collect();
        let csr = Csr::from_edges(100, &edges);
        let reached = reachability_gpu(&csr, &[0]);
        assert!(reached.iter().all(|&v| v), "all nodes should be reachable");
    }

    // ── Max Flow ─────────────────────────────────────────────────────────────

    #[test]
    fn max_flow_diamond_network() {
        // 0->1(3), 0->2(2), 1->2(1), 1->3(2), 2->3(4)
        // max flow = 5 (paths: 0->1->3 cap2, 0->2->3 cap2, 0->1->2->3 cap1)
        let edges = vec![(0,1,3i64),(0,2,2),(1,2,1),(1,3,2),(2,3,4)];
        let flow = max_flow_gpu(4, &edges, 0, 3);
        assert_eq!(flow, 5);
    }

    #[test]
    fn max_flow_single_edge() {
        let edges = vec![(0usize, 1usize, 7i64)];
        let flow = max_flow_gpu(2, &edges, 0, 1);
        assert_eq!(flow, 7);
    }

    #[test]
    fn max_flow_parallel_paths() {
        // 0->1(5), 0->2(5), 1->3(5), 2->3(5) => max flow = 10
        let edges = vec![(0,1,5i64),(0,2,5),(1,3,5),(2,3,5)];
        let flow = max_flow_gpu(4, &edges, 0, 3);
        assert_eq!(flow, 10);
    }

    #[test]
    fn max_flow_bottleneck() {
        // 0->1(100), 1->2(1), 2->3(100) => max flow = 1
        let edges = vec![(0,1,100i64),(1,2,1),(2,3,100)];
        let flow = max_flow_gpu(4, &edges, 0, 3);
        assert_eq!(flow, 1);
    }

    // ── AC-3 ─────────────────────────────────────────────────────────────────

    #[test]
    fn ac3_neq_prunes_nothing_when_domains_disjoint_not_needed() {
        // X0 in {1,2,3}, X1 in {1,2,3}, constraint X0 != X1
        // AC-3: every value in X0 has support in X1 (since |D|>1), no pruning
        let domains = vec![vec![1, 2, 3], vec![1, 2, 3]];
        let mut cg = ConstraintGraph::default();
        cg.add_constraint(0, 1, |a, b| a != b);
        let pruned = ac3_gpu_apply(&domains, &cg).unwrap();
        assert_eq!(pruned[0].len(), 3);
        assert_eq!(pruned[1].len(), 3);
    }

    #[test]
    fn ac3_forces_pruning_when_single_value_conflicts() {
        // X0 in {1}, X1 in {1,2}, constraint X0 != X1
        // AC-3 on arc (X1,X0): value 1 in X1 has no support -> pruned
        let domains = vec![vec![1i32], vec![1, 2]];
        let mut cg = ConstraintGraph::default();
        cg.add_constraint(0, 1, |a, b| a != b);
        cg.add_constraint(1, 0, |a, b| a != b);
        let pruned = ac3_gpu_apply(&domains, &cg).unwrap();
        // X1 should have 1 removed, leaving {2}
        assert!(!pruned[1].contains(&1), "value 1 should be pruned from X1");
        assert!(pruned[1].contains(&2));
    }

    // ── Forward Checking ─────────────────────────────────────────────────────

    #[test]
    fn forward_check_assignment_prunes_conflicting_values() {
        // X0={1,2,3}, X1={1,2,3}, constraint X0!=X1, assign X0=1
        // -> X1 should have 1 removed
        let domains = vec![vec![1, 2, 3], vec![1, 2, 3]];
        let mut cg = ConstraintGraph::default();
        cg.add_constraint(0, 1, |a, b| a != b);
        let assignment = vec![Some(1), None];
        let gpu_buf = forward_check_gpu_build(&domains, &assignment, &cg).unwrap();
        let pruned = gpu_buf.to_domains();
        assert!(!pruned[1].contains(&1), "value 1 should be pruned from X1");
        assert!(pruned[1].contains(&2));
        assert!(pruned[1].contains(&3));
    }

    #[test]
    fn forward_check_no_assignment_leaves_domains_intact() {
        let domains = vec![vec![1, 2], vec![1, 2]];
        let mut cg = ConstraintGraph::default();
        cg.add_constraint(0, 1, |a, b| a != b);
        let assignment = vec![None, None];
        let gpu_buf = forward_check_gpu_build(&domains, &assignment, &cg).unwrap();
        let pruned = gpu_buf.to_domains();
        assert_eq!(pruned[0].len(), 2);
        assert_eq!(pruned[1].len(), 2);
    }

    // ── Bitonic Sort ─────────────────────────────────────────────────────────

    #[test]
    fn bitonic_sort_ascending_order() {
        let mut arr = vec![9i64, 3, 7, 1, 5, 8, 2, 6, 4, 0];
        bitonic_sort_gpu(&mut arr);
        let expected: Vec<i64> = (0..10).collect();
        assert_eq!(arr, expected);
    }

    #[test]
    fn bitonic_sort_already_sorted() {
        let mut arr: Vec<i64> = (0..16).collect();
        bitonic_sort_gpu(&mut arr);
        let expected: Vec<i64> = (0..16).collect();
        assert_eq!(arr, expected);
    }

    #[test]
    fn bitonic_sort_reverse_sorted() {
        let n = 1024usize;
        let mut arr: Vec<i64> = (0..n as i64).rev().collect();
        bitonic_sort_gpu(&mut arr);
        for i in 0..n - 1 {
            assert!(arr[i] <= arr[i + 1], "not sorted at index {}", i);
        }
        assert_eq!(arr[0], 0);
        assert_eq!(arr[n - 1], n as i64 - 1);
    }

    #[test]
    fn bitonic_sort_non_power_of_two_length() {
        // wrapper pads internally
        let mut arr = vec![5i64, 1, 4, 2, 8];
        bitonic_sort_gpu(&mut arr);
        assert_eq!(arr, vec![1, 2, 4, 5, 8]);
    }

    // ── Linear Search ────────────────────────────────────────────────────────

    #[test]
    fn linear_search_found_at_correct_index() {
        let arr: Vec<i64> = (0..20).collect();
        assert_eq!(linear_search_gpu(&arr, 13), Some(13));
    }

    #[test]
    fn linear_search_target_not_present() {
        let arr: Vec<i64> = (0..20).collect();
        assert_eq!(linear_search_gpu(&arr, 99), None);
    }

    #[test]
    fn linear_search_first_element() {
        let arr: Vec<i64> = (0..100).collect();
        assert_eq!(linear_search_gpu(&arr, 0), Some(0));
    }

    #[test]
    fn linear_search_last_element() {
        let arr: Vec<i64> = (0..100).collect();
        assert_eq!(linear_search_gpu(&arr, 99), Some(99));
    }

    #[test]
    fn linear_search_returns_first_occurrence() {
        // duplicate values: min index wins via atomicMin
        let arr = vec![5i64, 3, 5, 1, 5];
        assert_eq!(linear_search_gpu(&arr, 5), Some(0));
    }

    // ── Matrix Multiply ──────────────────────────────────────────────────────

    #[test]
    fn matmul_identity_times_identity() {
        #[rustfmt::skip]
        let id: Vec<i64> = vec![
            1, 0, 0,
            0, 1, 0,
            0, 0, 1,
        ];
        let c = matrix_multiply_gpu(&id, &id, 3);
        assert_eq!(c, id);
    }

    #[test]
    fn matmul_known_result() {
        // A = [[1,2],[3,4]], B = [[5,6],[7,8]]
        // C = [[19,22],[43,50]]
        let a = vec![1i64, 2, 3, 4];
        let b = vec![5i64, 6, 7, 8];
        let c = matrix_multiply_gpu(&a, &b, 2);
        assert_eq!(c, vec![19, 22, 43, 50]);
    }

    #[test]
    fn matmul_zero_matrix() {
        let zero = vec![0i64; 9];
        let id: Vec<i64> = vec![1,0,0, 0,1,0, 0,0,1];
        let c = matrix_multiply_gpu(&zero, &id, 3);
        assert_eq!(c, zero);
    }

    // ── Sieve ────────────────────────────────────────────────────────────────

    #[test]
    fn sieve_primes_up_to_50() {
        let primes = sieve_gpu(50);
        assert_eq!(primes, vec![2,3,5,7,11,13,17,19,23,29,31,37,41,43,47]);
    }

    #[test]
    fn sieve_prime_count_up_to_100() {
        // pi(100) = 25
        let primes = sieve_gpu(100);
        assert_eq!(primes.len(), 25);
    }

    #[test]
    fn sieve_prime_count_up_to_1000() {
        // pi(1000) = 168
        let primes = sieve_gpu(1000);
        assert_eq!(primes.len(), 168);
    }

    #[test]
    fn sieve_no_composites_in_output() {
        let primes = sieve_gpu(200);
        for &p in &primes {
            assert!(p >= 2, "{} < 2", p);
            for d in 2..p {
                assert_ne!(p % d, 0, "{} is not prime", p);
            }
        }
    }

    // ── Rabin-Karp ───────────────────────────────────────────────────────────

    #[test]
    fn rabin_karp_finds_two_occurrences() {
        let text    = b"abracadabra";
        let pattern = b"abra";
        let mut matches = rabin_karp_gpu(text, pattern);
        matches.sort_unstable();
        assert_eq!(matches, vec![0, 7]);
    }

    #[test]
    fn rabin_karp_no_match() {
        let text    = b"abracadabra";
        let pattern = b"xyz";
        assert_eq!(rabin_karp_gpu(text, pattern), vec![]);
    }

    #[test]
    fn rabin_karp_full_string_match() {
        let text    = b"hello";
        let pattern = b"hello";
        assert_eq!(rabin_karp_gpu(text, pattern), vec![0]);
    }

    #[test]
    fn rabin_karp_overlapping_pattern() {
        // "aaaa" contains "aa" at positions 0, 1, 2
        let text    = b"aaaa";
        let pattern = b"aa";
        let mut matches = rabin_karp_gpu(text, pattern);
        matches.sort_unstable();
        assert_eq!(matches, vec![0, 1, 2]);
    }

    // ── Merkle Tree ──────────────────────────────────────────────────────────

    #[test]
    fn merkle_root_is_nonzero_for_nonzero_pages() {
        let pages = vec![0xabu8; 4 * PAGE_SIZE];
        let tree = merkle_build_gpu(&pages);
        let r = root(&tree);
        assert_eq!(r.len(), 32);
        assert!(r.iter().any(|&b| b != 0), "root should be nonzero");
    }

    #[test]
    fn merkle_root_length_is_32() {
        let pages = vec![0u8; 2 * PAGE_SIZE];
        let tree = merkle_build_gpu(&pages);
        assert_eq!(root(&tree).len(), 32);
    }

    #[test]
    fn merkle_different_pages_produce_different_roots() {
        let pages_a = vec![0xaau8; 4 * PAGE_SIZE];
        let pages_b = vec![0xbbu8; 4 * PAGE_SIZE];
        let tree_a = merkle_build_gpu(&pages_a);
        let tree_b = merkle_build_gpu(&pages_b);
        assert_ne!(root(&tree_a), root(&tree_b));
    }

    #[test]
    fn merkle_same_pages_produce_same_root() {
        let pages = vec![0x42u8; 4 * PAGE_SIZE];
        let tree_a = merkle_build_gpu(&pages);
        let tree_b = merkle_build_gpu(&pages);
        assert_eq!(root(&tree_a), root(&tree_b));
    }

    #[test]
    fn merkle_tree_buffer_size_correct() {
        let l = 4usize;
        let pages = vec![0u8; l * PAGE_SIZE];
        let tree = merkle_build_gpu(&pages);
        // 2*L nodes * 32 bytes each
        assert_eq!(tree.len(), 2 * l * 32);
    }

    // ── Bellman-Ford ─────────────────────────────────────────────────────────

    #[test]
    fn bellman_ford_known_shortest_paths() {
        // 0->1(4), 0->2(1), 2->1(2), 1->3(1)
        // dist from 0: [0, 3, 1, 4]
        let edges = vec![(0usize, 1usize, 4u64), (0, 2, 1), (2, 1, 2), (1, 3, 1)];
        let dist = bellman_ford_gpu(4, &edges, 0).unwrap();
        assert_eq!(dist, vec![0, 3, 1, 4]);
    }

    #[test]
    fn bellman_ford_single_edge() {
        let edges = vec![(0usize, 1usize, 7u64)];
        let dist = bellman_ford_gpu(2, &edges, 0).unwrap();
        assert_eq!(dist[0], 0);
        assert_eq!(dist[1], 7);
    }

    #[test]
    fn bellman_ford_disconnected_node_is_max() {
        // 0->1(1), node 2 unreachable from 0
        let edges = vec![(0usize, 1usize, 1u64)];
        let dist = bellman_ford_gpu(3, &edges, 0).unwrap();
        assert_eq!(dist[0], 0);
        assert_eq!(dist[1], 1);
        assert_eq!(dist[2], u64::MAX / 2);
    }

    #[test]
    fn bellman_ford_linear_chain() {
        // 0->1(1)->2(1)->3(1)->4(1), dist = [0,1,2,3,4]
        let edges: Vec<(usize, usize, u64)> = (0..4).map(|i| (i, i + 1, 1u64)).collect();
        let dist = bellman_ford_gpu(5, &edges, 0).unwrap();
        assert_eq!(dist, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn bellman_ford_source_is_last_node() {
        // source = 3, no outgoing edges from 3 in this graph
        // 0->1(2), 1->2(2); source 3 -> only dist[3]=0, rest=MAX
        let edges = vec![(0usize, 1usize, 2u64), (1, 2, 2)];
        let dist = bellman_ford_gpu(4, &edges, 3).unwrap();
        assert_eq!(dist[3], 0);
        assert_eq!(dist[0], u64::MAX / 2);
        assert_eq!(dist[1], u64::MAX / 2);
        assert_eq!(dist[2], u64::MAX / 2);
    }

    // ── Dominators ───────────────────────────────────────────────────────────

    #[test]
    fn dominators_diamond_cfg() {
        // CFG: 0->1, 0->2, 1->3, 2->3  (pred edges reversed for dominator input)
        // dom(0)={0}, dom(1)={0,1}, dom(2)={0,2}, dom(3)={0,3}
        let pred_csr = Csr::from_adj(&vec![vec![], vec![0], vec![0], vec![1, 2]]);
        let n = 4usize;
        let words = (n + 63) / 64;
        let dom_bits = dominators_gpu(&pred_csr, 0, n);

        let dom_set = |node: usize| -> Vec<usize> {
            (0..n).filter(|&i| {
                let word = dom_bits[node * words + (i >> 6)];
                (word >> (i & 63)) & 1 == 1
            }).collect()
        };

        assert_eq!(dom_set(0), vec![0]);
        assert_eq!(dom_set(1), vec![0, 1]);
        assert_eq!(dom_set(2), vec![0, 2]);
        assert_eq!(dom_set(3), vec![0, 3]);
    }

    #[test]
    fn dominators_linear_chain() {
        // 0->1->2->3; dom(k) = {0..=k}
        let pred_csr = Csr::from_adj(&vec![vec![], vec![0], vec![1], vec![2]]);
        let n = 4usize;
        let words = (n + 63) / 64;
        let dom_bits = dominators_gpu(&pred_csr, 0, n);

        for node in 0..n {
            for d in 0..n {
                let word = dom_bits[node * words + (d >> 6)];
                let bit = (word >> (d & 63)) & 1 == 1;
                if d <= node {
                    assert!(bit, "node {} should be dominated by {}", node, d);
                } else {
                    assert!(!bit, "node {} should NOT be dominated by {}", node, d);
                }
            }
        }
    }

    #[test]
    fn dominators_entry_dominates_all() {
        // Entry node 0 must dominate every reachable node regardless of topology
        let pred_csr = Csr::from_adj(&vec![vec![], vec![0], vec![0], vec![1, 2]]);
        let n = 4usize;
        let words = (n + 63) / 64;
        let dom_bits = dominators_gpu(&pred_csr, 0, n);

        for node in 0..n {
            let word = dom_bits[node * words];
            assert!((word & 1) == 1, "entry 0 must dominate node {}", node);
        }
    }

    // ── Reaching Definitions ─────────────────────────────────────────────────

    #[test]
    fn reaching_defs_linear_chain_propagates() {
        // Blocks 0->1->2; d0 defined in block 0, d1 in block 1
        // out(0)={d0}, out(1)={d0,d1}, out(2)={d0,d1}
        let pred_csr = Csr::from_adj(&vec![vec![], vec![0], vec![1]]);
        let block_count = 3;
        let def_count = 2;
        let words = (def_count + 63) / 64;
        let mut r#gen = vec![0u64; block_count * words];
        let kill = vec![0u64; block_count * words];
        r#gen[0] |= 1u64 << 0; // block 0 generates d0
        r#gen[1] |= 1u64 << 1; // block 1 generates d1
        let out = reaching_definitions_gpu(&pred_csr, block_count, def_count, &r#gen, &kill);

        let def_set = |b: usize| -> Vec<usize> {
            (0..def_count).filter(|&i| {
                let word = out[b * words + (i >> 6)];
                (word >> (i & 63)) & 1 == 1
            }).collect()
        };

        assert_eq!(def_set(0), vec![0]);
        assert_eq!(def_set(1), vec![0, 1]);
        assert_eq!(def_set(2), vec![0, 1]);
    }

    #[test]
    fn reaching_defs_kill_blocks_propagation() {
        // Blocks 0->1->2; d0 defined in 0, block 1 kills d0 and defines d1
        // out(0)={d0}, out(1)={d1}, out(2)={d1}
        let pred_csr = Csr::from_adj(&vec![vec![], vec![0], vec![1]]);
        let block_count = 3;
        let def_count = 2;
        let words = (def_count + 63) / 64;
        let mut r#gen = vec![0u64; block_count * words];
        let mut kill = vec![0u64; block_count * words];
        r#gen[0] |= 1u64 << 0;
        r#gen[1] |= 1u64 << 1;
        kill[1] |= 1u64 << 0; // block 1 kills d0
        let out = reaching_definitions_gpu(&pred_csr, block_count, def_count, &r#gen, &kill);

        let def_set = |b: usize| -> Vec<usize> {
            (0..def_count).filter(|&i| {
                let word = out[b * words + (i >> 6)];
                (word >> (i & 63)) & 1 == 1
            }).collect()
        };

        assert_eq!(def_set(0), vec![0]);
        assert_eq!(def_set(1), vec![1]);
        assert_eq!(def_set(2), vec![1]);
    }

    // ── Model Checking ───────────────────────────────────────────────────────

    #[test]
    fn model_check_all_reachable_states_satisfy_invariant() {
        // diamond graph, all nodes marked valid
        let csr = diamond_csr();
        let invariant = vec![1u8; 4];
        assert!(model_check_gpu(&csr, &[0usize], &invariant));
    }

    #[test]
    fn model_check_reachable_violation_fails() {
        // diamond graph, node 3 is reachable and violates invariant
        let csr = diamond_csr();
        let mut invariant = vec![1u8; 4];
        invariant[3] = 0;
        assert!(!model_check_gpu(&csr, &[0usize], &invariant));
    }

    #[test]
    fn model_check_unreachable_violation_passes() {
        // 0->1, node 2 unreachable; invariant violated only at node 2
        let csr = Csr::from_edges(3, &[(0, 1)]);
        let mut invariant = vec![1u8; 3];
        invariant[2] = 0;
        assert!(model_check_gpu(&csr, &[0usize], &invariant));
    }

    #[test]
    fn model_check_empty_reachable_set_passes() {
        // source=1 in a graph with no edges from 1; only node 1 reachable
        let csr = Csr::from_edges(3, &[(0, 2)]);
        let invariant = vec![1u8; 3];
        assert!(model_check_gpu(&csr, &[1usize], &invariant));
    }

    // ── CsrUnified ───────────────────────────────────────────────────────────

    #[test]
    fn csr_unified_row_ptr_matches_adj() {
        let mut g = AdjList::new(4);
        g.add_edge(0, 1);
        g.add_edge(0, 2);
        g.add_edge(1, 3);
        g.add_edge(2, 3);
        let ucsr = CsrUnified::from_adj(&g);
        let csr  = g.to_csr();
        assert_eq!(ucsr.row_ptr_slice(), csr.row_ptr.as_slice());
        assert_eq!(ucsr.col_idx_slice(), csr.col_idx.as_slice());
    }

    #[test]
    fn csr_unified_bfs_agrees_with_csr_bfs() {
        use crate::graph::csr::Csr as CsrStd;
        let mut g = AdjList::new(4);
        g.add_edge(0, 1);
        g.add_edge(0, 2);
        g.add_edge(1, 3);
        g.add_edge(2, 3);
        let ucsr = CsrUnified::from_adj(&g);
        let csr2 = CsrStd { row_ptr: ucsr.row_ptr_slice().to_vec(), col_idx: ucsr.col_idx_slice().to_vec() };
        let levels_std     = bfs_gpu(&g.to_csr(), 0);
        let levels_unified = bfs_gpu(&csr2, 0);
        assert_eq!(levels_std, levels_unified);
    }
}

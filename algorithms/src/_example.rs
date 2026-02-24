//! Standalone runner that demonstrates examples for all algorithms in `algorithms/src`

mod computation_map;
mod graph;
mod optimization;

use computation_map::ALGORITHMS_COMPUTATION_MAP;

fn main() {
    println!("=== Algorithms Computation Map ===");
    for (path, comp_type, determinism) in ALGORITHMS_COMPUTATION_MAP.iter() {
        println!("{:<50} | {:<25} | {}", path, comp_type, determinism);
    }

    println!("\n=== Sanity Check Examples ===");

    // Graph algorithms examples
    {
        use graph::bfs::bfs;
        use graph::dfs::dfs;
        use graph::dijkstra::dijkstra;
        use graph::topological_sort::topological_sort;
        use graph::invariant::{Invariant, NonNegative};

        let non_negative = NonNegative;

        let adj = vec![vec![1,2], vec![3], vec![3], vec![]];
        println!("BFS example: {:?}", bfs(&adj, 0));
        println!("DFS example: {:?}", dfs(&adj, 0));
        println!("Topological sort example: {:?}", topological_sort(&adj));

        let weighted_adj = vec![vec![(1,4),(2,1)], vec![(3,1)], vec![(1,2),(3,5)], vec![]];
        let dijkstra_result = dijkstra(&weighted_adj, 0);
        assert!(dijkstra_result.iter().all(|&x| non_negative.check(&(x as i64))));
        println!("Dijkstra example: {:?}", dijkstra_result);
    }

    // Optimization algorithms examples
    {
        use optimization::a_star::a_star;
        use optimization::backtracking::subsets;
        use optimization::branch_and_bound::branch_and_bound;
        use optimization::genetic_algorithm::genetic_optimize;

        let opt_adj = vec![vec![(1,1),(2,4)], vec![(2,2),(3,6)], vec![(3,3)], vec![]];
        let heuristic = |_: usize| 0;
        if let Some(cost) = a_star(&opt_adj, 0, 3, heuristic) {
            println!("A* example: {:?}", cost);
        }

        let set = vec![1,2,3];
        println!("Backtracking subsets example: {:?}", subsets(&set));

        let weights = vec![2,3,4];
        let capacity = 5;
        println!("Branch and Bound example: {:?}", branch_and_bound(&weights, capacity));

        let population = vec![1,5,10,7,3];
        let fitness = |x:u64| x;
        println!("Genetic algorithm example: {:?}", genetic_optimize(population, fitness, 10));
    }
}

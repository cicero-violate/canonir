// _example.rs: standalone runnable examples for graph module with invariant checks

mod bfs;
mod dfs;
mod dijkstra;
mod topological_sort;
mod invariant;

use bfs::bfs;
use dfs::dfs;
use dijkstra::dijkstra;
use topological_sort::topological_sort;
use invariant::{Invariant, NonNegative};

fn main() {
    let non_negative = NonNegative;

    // Example unweighted graph
    let graph = vec![
        vec![1, 2], // 0
        vec![3],    // 1
        vec![3],    // 2
        vec![],     // 3
    ];
    println!("BFS starting at 0: {:?}", bfs(&graph, 0));
    println!("DFS starting at 0: {:?}", dfs(&graph, 0));
    println!("Topological sort: {:?}", topological_sort(&graph));

    // Example weighted graph for Dijkstra
    let weighted_graph = vec![
        vec![(1, 4), (2, 1)], // 0
        vec![(3, 1)],         // 1
        vec![(1, 2), (3, 5)], // 2
        vec![],               // 3
    ];
    let dijkstra_res = dijkstra(&weighted_graph, 0);
    assert!(dijkstra_res.iter().all(|&x| non_negative.check(&(x as i64))));
    println!("Dijkstra shortest paths from 0: {:?}", dijkstra_res);
}

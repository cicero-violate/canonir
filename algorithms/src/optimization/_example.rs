mod a_star;
mod backtracking;
mod branch_and_bound;
mod genetic_algorithm;
mod invariant;

use a_star::a_star;
use backtracking::subsets;
use branch_and_bound::branch_and_bound;
use genetic_algorithm::genetic_optimize;
use invariant::{Invariant, NonNegative};

fn main() {
    // A* example
    let adj = vec![
        vec![(1, 1), (2, 4)],
        vec![(2, 2), (3, 6)],
        vec![(3, 3)],
        vec![],
    ];
    let heuristic = |_: usize| 0;
    if let Some(cost) = a_star(&adj, 0, 3, heuristic) {
        assert!(NonNegative.check(&(cost as i64)));
        println!("A* from 0 to 3: {:?}", cost);
    }

    // Backtracking example
    let set = vec![1, 2, 3];
    let all_subsets = subsets(&set);
    println!("Subsets of {:?}: {:?}", set, all_subsets);

    // Branch and Bound example
    let weights = vec![2, 3, 4];
    let capacity = 5;
    let bb_result = branch_and_bound(&weights, capacity);
    assert!(NonNegative.check(&(bb_result as i64)));
    println!("Branch and Bound max weight: {}", bb_result);

    // Genetic Algorithm example
    let population = vec![1, 5, 10, 7, 3];
    let fitness = |x: u64| x;
    let ga_best = genetic_optimize(population, fitness, 10);
    assert!(NonNegative.check(&(ga_best as i64)));
    println!("Genetic algorithm best: {}", ga_best);
}

use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Copy, Clone, Eq, PartialEq)]
struct Node {
    cost: u64,
    pos: usize,
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.cmp(&self.cost)
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn a_star(adj: &[Vec<(usize, u64)>], start: usize, goal: usize, heuristic: fn(usize) -> u64) -> Option<u64> {
    let mut dist = vec![u64::MAX; adj.len()];
    let mut heap = BinaryHeap::new();

    dist[start] = 0;
    heap.push(Node { cost: heuristic(start), pos: start });

    while let Some(Node { pos, .. }) = heap.pop() {
        if pos == goal {
            return Some(dist[pos]);
        }
        for &(next, weight) in &adj[pos] {
            let next_cost = dist[pos] + weight;
            if next_cost < dist[next] {
                dist[next] = next_cost;
                heap.push(Node { cost: next_cost + heuristic(next), pos: next });
            }
        }
    }
    None
}

//! # Algorithms Crate
//!
//! Foundational algorithm library organized by category.
//!
//! ## Modules
//!
//! - `control_flow` – State transition primitives (sequential, branching, loops, recursion)
//! - `data_structures` – Core structural containers (array, stack, queue, heap, hash table)
//! - `searching` – Lookup algorithms (linear, binary, hash-based)
//! - `sorting` – Ordering algorithms (merge, quick, heap)
//! - `graph` – Graph traversal & pathfinding (BFS, DFS, Dijkstra, Topological sort)
//! - `dynamic_programming` – Memoization and tabulation strategies
//! - `string_algorithms` – Pattern matching & indexing (KMP, Rabin–Karp, Trie, Suffix array)
//! - `numerical` – Mathematical algorithms (GCD, fast power, sieve, matrix multiply)
//! - `parsing_compilation` – AST, parsing, type checking, finite automata
//! - `memory_systems` – Memory management models (LRU, mark-sweep, ref counting)
//! - `concurrency` – Synchronization primitives (mutex, semaphore, CAS, actors)
//! - `optimization` – Search & optimization (backtracking, branch & bound, A*, genetic)
//! - `cryptography` – Hashing & integrity (Merkle tree, SHA-256)
//!
//! ---
//!
//! ## Usage Example
//!
//! ```rust
//! use algorithms::sorting::merge_sort::merge_sort;
//!
//! let sorted = merge_sort(&[3,1,2]);
//! assert_eq!(sorted, vec![1,2,3]);
//! ```
//!
//! ---
//!
//! Designed as a structured computational foundation layer.

pub mod concurrency;
pub mod control_flow;
pub mod cryptography;
pub mod data_structures;
pub mod dynamic_programming;
pub mod graph;
pub mod memory_systems;
pub mod numerical;
pub mod optimization;
pub mod parsing_compilation;
pub mod searching;
pub mod sorting;
pub mod string_algorithms;

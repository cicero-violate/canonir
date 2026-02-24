/// Mapping of modules in algorithms/src to type of computation
/// and whether deterministic or stochastic
pub const ALGORITHMS_COMPUTATION_MAP: &[(&str, &str, &str)] = &[
    // Graph algorithms
    ("graph/bfs.rs", "Graph traversal", "Deterministic"),
    ("graph/dfs.rs", "Graph traversal", "Deterministic"),
    ("graph/dijkstra.rs", "Shortest path", "Deterministic"),
    (
        "graph/topological_sort.rs",
        "Topological ordering",
        "Deterministic",
    ),
    // Optimization
    (
        "optimization/a_star.rs",
        "Heuristic search",
        "Deterministic",
    ),
    (
        "optimization/backtracking.rs",
        "Combinatorial search",
        "Deterministic",
    ),
    (
        "optimization/branch_and_bound.rs",
        "Combinatorial search",
        "Deterministic",
    ),
    (
        "optimization/genetic_algorithm.rs",
        "Stochastic optimization",
        "Stochastic",
    ),
    // Dynamic programming
    (
        "dynamic_programming/memoization.rs",
        "DP computation",
        "Deterministic",
    ),
    (
        "dynamic_programming/tabulation.rs",
        "DP computation",
        "Deterministic",
    ),
    // Sorting
    ("sorting/heap_sort.rs", "Sorting", "Deterministic"),
    ("sorting/merge_sort.rs", "Sorting", "Deterministic"),
    ("sorting/quick_sort.rs", "Sorting", "Deterministic"),
    // Searching
    ("searching/binary_search.rs", "Search", "Deterministic"),
    ("searching/linear_search.rs", "Search", "Deterministic"),
    ("searching/hash_lookup.rs", "Search", "Deterministic"),
    // Data structures
    (
        "data_structures/array.rs",
        "Data structure operations",
        "Deterministic",
    ),
    (
        "data_structures/linked_list.rs",
        "Data structure operations",
        "Deterministic",
    ),
    (
        "data_structures/stack.rs",
        "Data structure operations",
        "Deterministic",
    ),
    (
        "data_structures/queue.rs",
        "Data structure operations",
        "Deterministic",
    ),
    (
        "data_structures/heap.rs",
        "Data structure operations",
        "Deterministic",
    ),
    (
        "data_structures/hash_table.rs",
        "Data structure operations",
        "Deterministic",
    ),
    // Control flow
    ("control_flow/branching.rs", "Control flow", "Deterministic"),
    ("control_flow/looping.rs", "Control flow", "Deterministic"),
    ("control_flow/recursion.rs", "Control flow", "Deterministic"),
    (
        "control_flow/sequential.rs",
        "Control flow",
        "Deterministic",
    ),
    // Concurrency
    (
        "concurrency/actor_model.rs",
        "Concurrent computation",
        "Deterministic",
    ),
    ("concurrency/cas.rs", "Atomic operation", "Deterministic"),
    ("concurrency/mutex.rs", "Synchronization", "Deterministic"),
    (
        "concurrency/semaphore.rs",
        "Synchronization",
        "Deterministic",
    ),
    // Numerical
    (
        "numerical/fast_exponentiation.rs",
        "Numerical computation",
        "Deterministic",
    ),
    ("numerical/gcd.rs", "Numerical computation", "Deterministic"),
    (
        "numerical/matrix_multiplication.rs",
        "Numerical computation",
        "Deterministic",
    ),
    (
        "numerical/sieve.rs",
        "Numerical computation",
        "Deterministic",
    ),
    // String algorithms
    (
        "string_algorithms/kmp.rs",
        "String pattern matching",
        "Deterministic",
    ),
    (
        "string_algorithms/rabin_karp.rs",
        "String pattern matching",
        "Deterministic",
    ),
    (
        "string_algorithms/suffix_array.rs",
        "String pattern matching",
        "Deterministic",
    ),
    (
        "string_algorithms/trie.rs",
        "String pattern matching",
        "Deterministic",
    ),
    // Memory systems
    (
        "memory_systems/lru.rs",
        "Memory management",
        "Deterministic",
    ),
    (
        "memory_systems/mark_sweep.rs",
        "Memory management",
        "Deterministic",
    ),
    (
        "memory_systems/reference_counting.rs",
        "Memory management",
        "Deterministic",
    ),
    (
        "memory_systems/round_robin.rs",
        "Memory scheduling",
        "Deterministic",
    ),
    // Parsing / Compilation
    ("parsing_compilation/ast.rs", "Parsing", "Deterministic"),
    (
        "parsing_compilation/finite_automaton.rs",
        "Parsing / automata",
        "Deterministic",
    ),
    (
        "parsing_compilation/recursive_descent.rs",
        "Parsing",
        "Deterministic",
    ),
    (
        "parsing_compilation/type_checking.rs",
        "Type checking",
        "Deterministic",
    ),
];

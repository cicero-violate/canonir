| Category                  | Algorithm / Primitive | Core Idea                   | Primary Use Cases                       |
| ------------------------- | --------------------- | --------------------------- | --------------------------------------- |
| **Control Flow**          | Sequential Execution  | Ordered state transitions   | Any deterministic program execution     |
|                           | Branching (If/Else)   | Boolean decision            | Validation, routing logic, state gating |
|                           | Loop (While/For)      | Repeated transformation     | Simulation, iteration over collections  |
|                           | Recursion             | Self-referential reduction  | Tree traversal, divide-and-conquer      |
| **Data Structures**       | Array Indexing        | O(1) address arithmetic     | Random access tables, buffers           |
|                           | Linked List Traversal | Pointer chaining            | Dynamic collections, streaming          |
|                           | Stack                 | LIFO discipline             | Function calls, undo systems, parsing   |
|                           | Queue                 | FIFO discipline             | Scheduling, BFS, pipelines              |
|                           | Hash Table            | Key → bucket mapping        | Caches, symbol tables, fast lookup      |
|                           | Heap (Priority Queue) | Ordered partial tree        | Scheduling, Dijkstra, top-k problems    |
| **Searching**             | Linear Search         | Scan all elements           | Small or unsorted data                  |
|                           | Binary Search         | Divide ordered range        | Fast lookup in sorted arrays            |
|                           | Hash Lookup           | Constant-time average       | Dictionaries, memoization               |
| **Sorting**               | Merge Sort            | Stable divide & merge       | External sorting, stable ordering       |
|                           | Quick Sort            | Partition by pivot          | General-purpose in-memory sort          |
|                           | Heap Sort             | Repeated max extraction     | In-place guaranteed O(n log n)          |
| **Graph Algorithms**      | DFS                   | Depth exploration           | Cycle detection, topological sort       |
|                           | BFS                   | Layer exploration           | Shortest path (unweighted), networking  |
|                           | Dijkstra              | Greedy relaxation           | Weighted shortest path                  |
|                           | Topological Sort      | Linear DAG ordering         | Build systems, dependency resolution    |
| **Dynamic Programming**   | Memoization           | Cache recursion             | Optimization, combinatorics             |
|                           | Tabulation            | Bottom-up table             | Knapsack, path counting                 |
| **String Algorithms**     | KMP                   | Failure function matching   | Efficient substring search              |
|                           | Rabin–Karp            | Rolling hash                | Plagiarism detection, streaming match   |
|                           | Trie                  | Prefix tree                 | Autocomplete, routing tables            |
|                           | Suffix Array/Tree     | Indexed substrings          | Genome search, compression              |
| **Numerical**             | Euclidean GCD         | Modulo reduction            | Cryptography, fraction simplification   |
|                           | Fast Exponentiation   | Logarithmic powering        | Cryptography, large powers              |
|                           | Sieve of Eratosthenes | Prime marking               | Number theory, crypto prep              |
|                           | Matrix Multiplication | Linear transform            | Graphics, ML, simulations               |
| **Parsing / Compilation** | Finite Automaton      | Token recognition           | Lexers, protocol parsers                |
|                           | Recursive Descent     | Grammar expansion           | Compilers, config parsers               |
|                           | AST Construction      | Structural representation   | Code transformation, analysis           |
|                           | Type Checking         | Constraint solving          | Compiler safety guarantees              |
| **Memory & Systems**      | Mark & Sweep          | Reachability tracing        | Garbage collection                      |
|                           | Reference Counting    | Ownership tracking          | Deterministic memory mgmt               |
|                           | LRU                   | Temporal locality heuristic | Caches, OS paging                       |
|                           | Round Robin           | Time slicing                | CPU scheduling                          |
| **Concurrency**           | Mutex                 | Mutual exclusion            | Shared state safety                     |
|                           | Semaphore             | Resource counting           | Thread pools, rate limiting             |
|                           | CAS                   | Atomic compare-swap         | Lock-free structures                    |
|                           | Actor Model           | Message isolation           | Distributed systems                     |
| **Optimization/Search**   | Backtracking          | Exhaustive search           | Constraint solving, puzzles             |
|                           | Branch & Bound        | Pruned search               | NP-hard optimization                    |
|                           | A*                    | Heuristic-guided path       | Routing, game AI                        |
|                           | Genetic Algorithm     | Evolutionary search         | Large search spaces                     |

| Category           | Algorithm                    | Variables | Equation                    | Explanation                                      | GPU Acceleratable | Status    |
| ------------------ | ---------------------------  | --------- | --------------------------- | ------------------------------------------------ | ----------------- | --------- |
| Invariant Checking | Assertion Checking           | (S, I)    | ( \forall s \in S : I(s) )  | Verify invariant predicates hold for every state | Potential         | Pending   |
| Invariant Checking | DFS Cycle Detection          | (G=(V,E)) | (cycle(G)=DFS(G))           | Detect cycles in dependency graphs               | Mostly No         | Completed |
| Invariant Checking | Kosaraju SCC                 | (G)       | (SCC(G))                    | Finds strongly connected components              | Potential         | Completed |
| Invariant Checking | Kahn Topological Sort        | (G)       | (topo(G))                   | Validates DAG ordering                           | Potential         | Completed |
| Invariant Checking | Model Checking               | (S, I)    | (S \models I)               | Verify invariants across state transitions       | Yes (GPU)         | Completed |
| Constraint Solving | SAT (DPLL)                   | (F(x))    | (F(x)=true)                 | Boolean satisfiability search                    | Mostly No         | Pending   |
| Constraint Solving | SAT (CDCL)                   | (F(x))    | (solve(F))                  | Clause learning SAT solver                       | Potential         | Pending   |
| Constraint Solving | SMT Solver                   | (F(x),T)  | (F(x)\land theory(x))       | SAT + theory reasoning                           | Potential         | Pending   |
| Constraint Solving | Forward Checking             | (x_i)     | (prune(D_i))                | Remove invalid future assignments                | Yes (GPU)         | Completed |
| Constraint Solving | Arc Consistency (AC-3)       | (x_i,x_j) | (revise(x_i,x_j))           | Maintain constraint consistency                  | Yes (GPU)         | Completed |
| Graph Reasoning    | Topological Scheduling       | (G=(V,E)) | (ready(v)\iff deps(v)=done) | Determine runnable tasks                         | Potential         | Completed |
| Graph Reasoning    | Shortest Path (Dijkstra)     | (G,w)     | (d(v)=min(d(u)+w))          | Optimal path computation                         | Potential         | Completed |
| Graph Reasoning    | Shortest Path (Bellman-Ford) | (G,w)     | (relax all edges)           | Handles negative weights                         | Yes (GPU)         | Completed |
| Graph Reasoning    | A* Search                    | (G,h)     | (f(n)=g(n)+h(n))            | Heuristic graph search                           | Mostly No         | Completed |
| Graph Reasoning    | Maximum Flow                 | (G,c)     | (max\sum f(s,v))            | Capacity-constrained flow optimization           | Yes (GPU)         | Completed |
| Graph Reasoning    | Reachability                 | (G)       | (reachable(u,v))            | Determine dependency connectivity                | Yes (GPU)         | Completed |
| Graph Reasoning    | BFS (GPU)                    | (G)       | (levels(v))                 | Parallel breadth-first traversal                 | Yes (GPU)         | Completed |

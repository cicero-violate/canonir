## PENDING
  1. GPU reachability used for unreachable set
     reachability_mask() already uses GPU, but it still rebuilds CSR on CPU every time. We can cache CSR to avoid repeated
     rebuilds.
  2. GPU batching (v3 Phase 10)
     Batch multiple graphs and run scheduler kernels once per batch to fully exploit GPU.
  3. Policy training loop automation
     train_policy.py exists, but there’s no automated training/refresh or evaluation.
  4. GPU tests for new feature kernels in canon-agent
     We added tests in algorithms; no integration test in canon-agent pipeline.
  5. Global failure recovery → planner replan
     v5 plan calls for a structural replan when failed_fraction is high. Current recovery only resets failed nodes; it does
     not force a planner replan.
  6. CSR caching across graph feature passes
     GPU feature extraction, topo, reachability, and indegree still rebuild CSR per call. Cache a CSR per graph snapshot to
     reduce CPU overhead.

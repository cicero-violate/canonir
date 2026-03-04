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
  5. CSR caching across graph feature passes
     GPU feature extraction, topo, reachability, and indegree still rebuild CSR per call. Cache a CSR per graph snapshot to
     reduce CPU overhead.
  6. GPU-side feature normalization
     normalize_features() runs on CPU; could be fused into GPU feature pipeline to avoid CPU work and extra copies.
  7. Train all policy heads
     Online training only updates planner_bias weights. node_add_bias, edge_add_bias, and rewrite_bias remain static.

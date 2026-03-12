# kernel/semantics

This directory contains **semantic analysis outputs** derived from the kernel graph.

## What Each File Means

- `upg_invariants.json`
  - **Invariant validation results** over the kernel graph.
  - Each invariant is a *rule* (predicate) about nodes/edges, with coverage and violations.

- `invariants_discovered.json`
  - **Heuristic invariants** mined from reports and structural patterns.
  - Not necessarily proven; used as candidates or hints.

- `invariant_candidates.json`
  - **Cluster-derived candidate invariants** (from semantic clustering + pattern mining).
  - Filtered by support/confidence thresholds.

- `invariant_validated.json`
  - **SAT-checked candidates** from `invariant_candidates.json`.
  - Marks candidates that survive validation.

- `semantic_clusters.json`
  - **Clusters of nodes** with similar structural signatures.
  - Clusters are *node groups*, not edges.

- `semantic_outliers.json`
  - **Node outliers** that did not cluster (often emission bugs or edge gaps).
  - Includes `node_id`, `symbol`, `file`, `line`, `kind`.

- `node_semantic_signatures.csv`
  - **Per-node structural signature** (hashed feature vector).
  - Used to drive clustering.

- `invariant_violations/`
  - One JSON file per failing invariant, listing violating nodes with symbols/files/lines.

## Quick Mental Model

- **Clusters** = groups of nodes.
- **Invariants** = rules about nodes/edges.
- **Outliers** = nodes that don’t fit learned structure.

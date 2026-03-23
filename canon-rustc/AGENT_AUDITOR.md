You are performing a hostile structural audit of the canon_kernel.

Goal:
Identify locations where the kernel can silently succeed while producing incorrect IR, missing edges, missing nodes, or suppressed failures.

This is NOT a correctness review.
This is a "find where the system can lie to us" review.

Focus only on structural failure surfaces.

---

Audit Targets

1. Silent Failure Paths
Search for:
- Option<T> returns where None is ignored
- Result<T> where Err is swallowed
- unwrap_or_default
- unwrap_or
- .ok()
- match _ => None
- early returns that skip emission

These create hidden data loss.

---

2. Incomplete MIR Coverage

Inspect these files:

capture/capture/mir/lower.rs
capture/capture/mir/expr.rs
capture/capture/mir/terminator.rs

Find:

- match statements over mir::Rvalue
- match statements over mir::StatementKind
- match statements over mir::TerminatorKind

Check if they use wildcard arms:

    _ => ...

Any wildcard handling means MIR constructs are silently dropped.

List every wildcard match.

---

3. Edge Emission Gaps

Inspect:

capture/capture/relations.rs
capture/capture/edge_emit.rs
capture/capture/engine.rs

Find cases where nodes are emitted but edges are optional.

Example failure:

node emitted
edge omitted

This corrupts graph topology.

---

4. Graph Structural Guarantees

Inspect:

capture/capture/validate/structural.rs

Verify:

- node_count == actual node rows
- edge_count == actual edges
- every edge src/dst node exists
- every function node has a body or explicit "external" tag

List missing invariants.

---

5. Panic Suppression

Inspect:

capture/capture/pipeline.rs
artifacts/logs.rs

Verify:

panic events propagate to:

Partial.panic_def_id

Ensure panics do NOT result in:

Partial { nodes: [], edges: [] }

because this hides structural failure.

---

6. Canon Assembly Integrity

Inspect:

capture/canon_assemble.rs

Look for:

- nodes inserted without stable ordering
- edges referencing missing ids
- implicit canonicalization of malformed paths

List every place where data is normalized or rewritten.

Normalization can hide capture errors.

---

7. Tlog Truthfulness

Inspect:

artifacts/tlog.rs

Check:

- write_node
- write_edge

Verify they never skip nodes or edges based on filtering rules.

Any filtering corrupts the append-only log.

---

Output Format

For each issue report:

TYPE: SilentDrop | CoverageGap | TopologyBreak | InvariantMissing | TelemetryLie

FILE:
FUNCTION:
LINE:

EXPLANATION:
Why this can cause the kernel to appear correct while emitting incorrect IR.

SEVERITY:
1 (low) – cosmetic
5 (critical) – corrupts graph or IR.

---

Important Rules

Do NOT suggest fixes.
Do NOT propose refactors.
Only locate structural risk.

Your task is to find where the kernel can lie to us.

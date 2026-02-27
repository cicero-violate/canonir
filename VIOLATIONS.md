# VIOLATIONS.md  
_Based on commit 8bf2f4d6041c463315dd03e65667d604ca1ee03c and prior diffs_

This document enumerates invariant violations currently being introduced across subsystems.

---

# 1. Capture Subsystem (canon-capture)

## ❌ Violation: String-Based Type Parsing Escalation

### Location
`canon_assemble.rs`  
Large expansion of:
- `normalize_type_text`
- `split_top_level`
- `split_generic_args`
- `parse_fn_ptr`
- manual tuple/array parsing

### Invariant Broken
> Capture must reflect rustc semantics, not reimplement a partial Rust parser.

### Why This Is a Violation
- Manual string parsing introduces grammar drift.
- Behavior is heuristic and order-sensitive.
- Lifetime handling, generics, nesting are partially replicated.
- Future rustc changes will silently desync Canon.

### Risk
- Non-deterministic IR shape.
- Incorrect TypeKind classification.
- Semantic divergence from HIR/MIR.

---

## ❌ Violation: Path Normalization via String Replace

### Location
`canon_assemble.rs`  
Pattern:
```

normalized.replace("Box<dyn {root}::", ...)
normalized.replace(", {root}::", ...)
normalized.replace("({root}::", ...)

```

### Invariant Broken
> Path normalization must be structural, not substring-based.

### Why This Is a Violation
- Replacement depends on formatting.
- Nested generics may partially rewrite.
- Non-token-aware replacement.
- Violates single-source-of-truth principle.

---

# 2. Use Solver (canon-analyzer/use_solver)

## ❌ Violation: Synthetic Node Injection in Solver Phase

### Location
`use_solver.rs`

```

ir.push_node(CanonNodeKind::Use { ... })

```

### Invariant Broken
> Solvers must not mutate structural topology.

### Why This Is a Violation
- Structural injection happens post-capture.
- Canon IR no longer reflects source.
- Projection behavior becomes solver-dependent.
- Order of solver execution now changes output.

### Risk
- Non-idempotent pipeline.
- IR != captured source.
- Hard to prove determinism.

---

## ❌ Violation: Target Backfilling on Use Nodes

```

*target = Some(CanonId(defs[0] as u32))

```

### Invariant Broken
> Name resolution edges must live in graphs, not node payloads.

### Why This Is a Violation
- CanonNodeKind now mixes structural + semantic data.
- Use node shape changes based on resolution pass.
- Dual source of truth (graph + node field).

---

# 3. Dependency Solver (dep_solver)

## ❌ Violation: Text-Scan Fallback

```

for text in &ir.name_intern.vec {
for token in text.split(...)

```

### Invariant Broken
> Dependencies must derive from structural graph, not intern text scan.

### Why This Is a Violation
- Reintroduces deleted infer_dependencies heuristic.
- Non-structural inference.
- Text scan ≠ semantic resolution.
- Prone to false positives.

---

# 4. Visibility Solver Repairs

## ❌ Violation: Silent Structural Mutation

```

*flags |= flags::PUB;
*flags &= !(flags::PUB | ...)

```

### Invariant Broken
> Solvers must validate invariants, not rewrite structure silently.

### Why This Is a Violation
- Capture emitted flags.
- Solver mutates flags post-hoc.
- Canon IR becomes phase-order dependent.

---

# 5. Path Normalization Drift in norm.rs

Tests modified:

```

assert_eq!(norm_path("data::model::User"), "data::model::User");

```

Previously:
```

crate::data::model::User

```

### Invariant Broken
> Canon path canonicalization must be globally consistent.

### Why This Is a Violation
- Behavior changed without schema versioning.
- Downstream solvers depend on path shape.
- Canon IR identity changes across commits.

---

# 6. Projection Layer Heuristic Dependency Mapping

`render_dependency_entry` special-cases:
```

tree_sitter → tree-sitter

```

### Invariant Broken
> Canon IR must not encode Cargo registry policy.

### Why This Is a Violation
- Projection embeds ecosystem-specific rewrite rules.
- Dependency solver becomes non-generic.
- Canon not ecosystem-agnostic.

---

# 7. Global Pattern Observed

Across commits:

| Category           | Pattern                                | Solution                                                                 |
|--------------------+----------------------------------------+--------------------------------------------------------------------------|
| Type handling      | String parsing instead of structural   | Emit/solve via `TypeId`/typed nodes only; forbid parsing type strings.   |
| Name resolution    | Node mutation instead of graph-only    | Keep CanonIR immutable; store resolves/aliases as edges + intern tables. |
| Dependencies       | Text scanning fallback                 | Derive deps from capture/IR edges; remove scan fallback; fail loudly.    |
| Visibility         | Silent repair                          | Make repairs explicit as `Fixup`/`Violation` records; require judgment.  |
| Path normalization | Multi-layer rewrite passes             | Single canonicalizer pass to a normalized `PathId`; no rewriters.        |
| Injection          | Solver generating new structural nodes | Solvers may only add edges/annotations; new nodes only from capture.     |

apply_patch <<'EOF'
*** Begin Patch
*** Update File: VIOLATIONS.md
@@
 # 7. Global Pattern Observed
 
 Across commits:

| Category           | Pattern                                | Solution                                                                                                                                                                                              |
|--------------------+----------------------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Type handling      | String parsing instead of structural   | Add structured `QualifiedPath` + `TypeRef/PathRef` nodes and represent generics/assoc paths structurally; restrict `TypeKind::Extern` to true externs only.                                           |
| Name resolution    | Node mutation instead of graph-only    | Introduce explicit `Unresolved*` states (e.g., `TypeKind::Unresolved(PathId)` / `Resolved(CanonId)`), and drive resolution by adding edges (`Resolves/Renames/Reexports`) rather than mutating nodes. |
| Dependencies       | Text scanning fallback                 | Emit `PathRef { path_id }` (and/or `ExternRef`) nodes from body capture for all external path uses; compute deps from `G_module/G_value/G_macro` + `PathRef` instead of scanning interned strings.    |
| Visibility         | Silent repair                          | Add `Visibility { kind, scope_path_id }` (or `flags + scope_path_id`) to encode `pub(in path)`; forbid solver “repair” by requiring capture to preserve the original restriction.                     |
| Path normalization | Multi-layer rewrite passes             | Replace raw `path_intern` strings with canonical structured paths (segments + qualifiers); enforce normalization at `intern_path` (single canonical form) before sealing.                             |
| Injection          | Solver generating new structural nodes | Make solvers append-only on *edges* (constraints) and emit “hints” separately; forbid creating new structural nodes post-seal except via a dedicated lowering pass with explicit provenance.          |


---

# Core Meta-Violation

Canon IR is drifting from:

> Capture → Pure structural IR → Solvers derive semantics

Toward:

> Capture + Heuristics + Mutation + Text Scan → Solver Repairs → Projection Fixups

This violates:

- Determinism
- Idempotence
- Structural Purity
- Phase Separation
- Single Source of Truth

---

# High Severity Violations

1. Solver injecting Use nodes
2. Text-based dependency fallback
3. Manual Rust grammar reimplementation
4. String-based path rewriting
5. Structural flag mutation in solver

---

# Required Correction Direction

- Remove solver structural mutations
- Remove text scanning
- Remove string rewrite normalization
- Move all structural truth into capture phase only
- Keep solvers graph-derivation only

---

# Conclusion

The system is currently compensating for incomplete structural capture with heuristic patches in:

- use_solver
- dep_solver
- canon_assemble normalization
- visibility_solver repairs

This is architectural debt accumulating inside Canon IR.

Structural purity is currently compromised.

This document defines the operating rules for any automated coding agent modifying the `canon_kernel`.

The kernel is **a truth-extraction system for Rust programs**.  
Any code change that hides errors, drops structure, or rewrites invalid data is considered a **critical violation**.

---

# Core Principle

The kernel must **never lie**.

All failures must be:

- detected
- preserved
- observable

Never hide or normalize incorrect data.

---

# Forbidden Patterns

## 1. Silent Drops

Never introduce logic that discards data silently.

Examples of forbidden patterns:

```

Option<T> → None without telemetry
Result<T> → Err ignored
unwrap_or_default
unwrap_or(...)
.ok()
continue that skips emission

```

If data cannot be emitted, the system must:

- record a failure
- preserve the context

---

## 2. Wildcard Pattern Matches

Never use wildcard matches for MIR constructs.

Forbidden:

```

match rvalue {
...
_ => ...
}

```

Forbidden:

```

match terminator {
...
_ => Terminator::None
}

```

Every MIR variant must be explicitly handled.

If unsupported, it must **panic with a structural invariant violation**.

---

## 3. Graph Topology Corruption

Nodes must never exist without their structural relationships.

Examples of forbidden states:

- node without parent `Contains` edge
- impl node without `ImplFor`
- function node without body or explicit external marker
- CFG block without terminator

If topology cannot be emitted, emit a **structural failure**.

---

## 4. Panic Suppression

Panics must **never disappear**.

Forbidden:

```

panic → empty Partial
panic → skip def
panic → silent drop

```

Panics must produce:

- a panic record
- preserved def_id
- backtrace

The kernel may continue execution, but the failure must remain visible.

---

## 5. Lossy Normalization

Normalization that hides invalid input is forbidden.

Examples:

- collapsing distinct paths
- rewriting malformed identifiers
- coercing incorrect types
- dropping macro fragments silently

Invalid data must surface as **errors**, not rewritten values.

---

# Required Guarantees

All code must maintain the following invariants.

## Node Invariants

Every node must have:

- stable ID
- valid symbol
- valid file reference
- consistent NodeKind

---

## Edge Invariants

Every edge must satisfy:

- src node exists
- dst node exists
- edge kind valid

No edge may reference missing nodes.

---

## MIR Coverage

Lowering must explicitly cover:

- `mir::Rvalue`
- `mir::StatementKind`
- `mir::TerminatorKind`

Any unhandled construct must produce a **panic invariant violation**.

---

## Telemetry Guarantees

Telemetry must reflect real system state.

Logs must never omit:

- panics
- missing edges
- invalid nodes
- lowering failures

Append-only logs must remain **complete and truthful**.

---

# Validation Requirements

Structural validation must ensure:

- node_count matches actual nodes
- edge_count matches actual edges
- all edge endpoints exist
- functions have body or external marker
- CSR graph integrity holds

Validation failures must abort kernel execution.

---

# Code Modification Rules

When modifying kernel code:

1. Never weaken invariants.
2. Never introduce silent data loss.
3. Never normalize invalid data.
4. Always preserve telemetry.
5. Prefer failing loudly over producing incorrect graphs.

---

# When in Doubt

If a construct cannot be safely lowered:

panic!("canon-kernel invariant violation: unsupported construct")


The kernel must fail **loudly and truthfully** rather than emit incorrect structure.
# Mission

The canon kernel is not a compiler.

It is a **truth extraction system for program structure**.

Correctness > completeness.

The system must always produce **truthful graphs**, even if that means failing on unsupported constructs.

rustc compiler source code can be found in here
/workspace/git_repos/rust-source

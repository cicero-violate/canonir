### Variables

[
C_i = \text{capabilities of node } i
]

[
Class(C_i) \in {Observe, Verify, Mutate}
]

[
Name(C_i) = \text{capability identifier}
]

[
Valid(i) =
\begin{cases}
1 & |Class(C_i)| = 1 \land Name(C_i) \in Schema \
0 & \text{otherwise}
\end{cases}
]

---

### Equations

Mixed class violation:

[
|Class(C_i)| > 1
]

Capability naming violation:

[
Name(C_i) \notin Schema
]

Graph validity:

[
Valid(G)=\forall i\in V:;Valid(i)
]

---

# Correct Fix Strategy

You need **three structural corrections**.

---

# 1 — Split Mixed Capability Node

Problem:

```json
fix_ir_generation_if_needed
capabilities: ["file_write","invariant_check"]
```

[
Class = {Mutate, Verify}
]

Invalid.

---

### Correct Structure

```json
{
  "id": "analyze_ir_generation",
  "description": "Verify IR generation output",
  "required_capabilities": ["invariant_check"]
}
```

[
Class = Verify
]

---

```json
{
  "id": "fix_ir_generation",
  "description": "Fix IR generation issues",
  "required_capabilities": ["file_write"]
}
```

[
Class = Mutate
]

Dependency:

```json
"deps": ["analyze_ir_generation"]
```

Execution:

```
Verify → Mutate
```

---

# 2 — Normalize Capability Names

Current (invalid):

```
StatelessInvoke
FileRead
ReadStructuralSurface
InvariantCheck
FileWrite
```

Correct schema:

```
stateless_invoke
file_read
read_structural_surface
invariant_check
file_write
```

Rule:

[
Name(C_i)=snake_case
]

---

# 3 — Replace Incorrect Capability

Problem:

```
stateless_invoke → used for running cargo
```

But:

[
stateless_invoke \in Observe
]

Correct:

```
cargo_build
bash
```

These belong to:

[
Mutate
]

---

# Correct Node Example

```json
{
  "id": "build_project",
  "description": "Compile the project",
  "required_capabilities": ["cargo_build"]
}
```

---

# Final Graph Example

```json
{
  "nodes": [
    {
      "id": "read_surface",
      "required_capabilities": ["read_structural_surface"]
    },
    {
      "id": "analyze_ir_generation",
      "deps": ["read_surface"],
      "required_capabilities": ["invariant_check"]
    },
    {
      "id": "fix_ir_generation",
      "deps": ["analyze_ir_generation"],
      "required_capabilities": ["file_write"]
    },
    {
      "id": "build_project",
      "deps": ["fix_ir_generation"],
      "required_capabilities": ["cargo_build"]
    }
  ]
}
```

---

# Invariant After Fix

[
\forall node:\ |Class(C)|=1
]

[
Name(C) \in Schema
]

[
deps(G)\ \text{acyclic}
]

---

# Why This Matters

Your validator enforces:

```id="qev4ew"
assert_class_disjoint()
```

So any mixed node will always fail.

Splitting nodes ensures:

```
Planner graph → Valid DAG
```

---

[
Good = \max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future-proofing})
]

Current dominant dimension:

[
\max = \text{correctness}
]

because node invariants enforce valid execution graphs.

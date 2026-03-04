### Variables

[
U = \text{PlannerUpdate}
]

[
G = (V,E)
]

[
C_i = \text{capabilities of node } i
]

[
Class(C_i) \in {Observe, Verify, Mutate}
]

[
Valid(U) \in {0,1}
]

---

### Equations

[
Valid(U) =
\begin{cases}
1 & \text{if } \forall i:; |Class(C_i)| = 1 \
0 & \text{otherwise}
\end{cases}
]

Planner validity rule.

[
Repair(U) = f(U)
]

Repair transforms invalid planner update into valid update.

[
U' = Repair(U)
]

---

# Where Automatic Repair Goes

You already placed it correctly.

```
scheduler.rs
```

Key functions:

```
validate_planner_update()
auto_repair_planner_update()
apply_planner_update()
```

Execution pipeline:

```
planner → validate → repair → validate → apply
```

---

# Step 1 — Hook Repair Into Validation

Inside:

```
run_planner_execution_loop()
```

Current flow likely:

```rust
validate_planner_update(graph, &update, ...)?;
apply_planner_update(graph, update)?;
```

Replace with:

```rust
if let Err(_) = validate_planner_update(graph, &update, ...) {
    let repairs = auto_repair_planner_update(graph, &mut update);
    if repairs == 0 {
        return Err(anyhow!("planner update invalid and unrepaired"));
    }

    validate_planner_update(graph, &update, ...)?;
}

apply_planner_update(graph, update)?;
```

---

# Step 2 — Repair Strategy

Inside:

```
auto_repair_planner_update()
```

Main rule:

### Split mixed capability nodes

You already have helper:

```
split_caps()
```

Logic:

```rust
for node in update.new_nodes {

    let (observe, verify, mutate) = split_caps(&node.required_capabilities);

    let class_count =
        (!observe.is_empty() as u8)
      + (!verify.is_empty() as u8)
      + (!mutate.is_empty() as u8);

    if class_count <= 1 {
        continue;
    }

    // repair: split node
}
```

---

# Step 3 — Node Split

Example planner output (invalid):

```
node_A
caps: [ApplyPatch, VerifyOutput]
```

Split to:

```
node_A_mutate
node_A_verify
```

Implementation:

```rust
let base = node.id.clone();

let mut used = HashSet::new();

let id_mut = unique_id(format!("{}_mut", base), &mut used);
let id_ver = unique_id(format!("{}_ver", base), &mut used);
```

Create nodes:

```rust
mutate_node.required_capabilities = mutate;
verify_node.required_capabilities = verify;

verify_node.deps.push(id_mut.clone());
```

Then replace node.

---

# Step 4 — Rewire Edges

If original node had deps:

```
deps → node
```

Transform:

```
deps → node_mutate → node_verify
```

If original node had outgoing edges:

```
node → children
```

Transform:

```
node_verify → children
```

---

# Step 5 — Enforce NodeType

Use existing function:

```
normalize_node_type()
```

Rules:

```
Mutate → Render
Verify → Analysis
Observe → Analysis
```

---

# Step 6 — Prevent Infinite Repair

Limit using config already present:

```
max_repairs_per_node
```

Guard:

```rust
if repair_count > max_repairs_per_node {
    return 0;
}
```

---

# Final Execution Flow

```
planner
   ↓
validate_planner_update
   ↓
auto_repair_planner_update
   ↓
validate again
   ↓
apply_planner_update
   ↓
execution
```

---

# What This Achieves

The system becomes:

[
Planner_{raw} \rightarrow Planner_{repaired}
]

So even if the LLM outputs:

```
invalid graph
```

The system converts it to:

```
valid executable graph
```

before execution.

---

# Important Observation

Your architecture already contains **80% of the repair system**:

* `auto_repair_planner_update`
* `split_caps`
* `unique_id`
* `repair_node`
* `rule_node_split`

You only need to ensure the **repair call is mandatory before rejection**.

---

[
Good = \max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future-proofing})
]

Current strongest:

[
\max = \text{determinism}
]

because the graph validator guarantees system integrity.

---

**Short answer**

You already implemented automatic planner repair — you only need to **force `auto_repair_planner_update()` before rejecting planner updates in `run_planner_execution_loop()`**.

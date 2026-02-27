## Capture Refactor Model

### Variables

* ( G_s ) = rustc graph
* ( G_c ) = Canon graph
* ( \pi ) = capture transform
* ( E ) = engine
* ( R ) = rule table
* ( M ) = MIR projection
* ( V ) = structural validator
* ( F ) = CanonFragment

---

### Equations

1. **New Capture Definition**
   [
   \pi = Assemble \circ Merge \circ E
   ]

2. **Engine**
   [
   E(def) = Apply(R, def) + M(def)
   ]

3. **Invariant Check**
   [
   V(G_c) = true
   ]

4. **Separation**
   [
   Engine \perp MIR \perp Validation
   ]

---

# Canon-Capture Refactor (Clean Architecture)

## Target Directory Structure

```text
canon-capture/
│
├── lib.rs
├── index.rs
├── norm.rs
│
├── capture/
│   ├── mod.rs
│   ├── pipeline.rs        // top-level orchestration
│   ├── engine.rs          // rule dispatcher
│   ├── rules.rs           // declarative RuleSpec
│   ├── relations.rs       // relation templates only
│   ├── fragments.rs       // CanonFragment + builders
│   │
│   ├── mir/
│   │   ├── mod.rs
│   │   ├── lower.rs       // mir_body_structural (CFG walker)
│   │   ├── patterns.rs    // MirPattern table
│   │   ├── guard.rs       // structural_guard logic
│   │   └── resolver.rs    // LocalNameResolver
│   │
│   ├── validate/
│   │   ├── mod.rs
│   │   └── structural.rs  // emission invariants
│   │
│   └── helpers.rs         // type + generics mapping
```

---

# Layer Responsibilities

## 1️⃣ pipeline.rs

```text
capture(tcx):
    index = build_index(tcx)
    fragments = []
    for def in index.def_ids:
        fragments += engine::lower_def(...)
    canon = canon_assemble(...)
    validate::structural(canon)
    return canon
```

Pure orchestration.

---

## 2️⃣ engine.rs

Only:

* analyze_def
* select_rule
* lower_def

No MIR logic.
No structural guards.

---

## 3️⃣ mir/lower.rs

Only:

[
CFG_{mir} \rightarrow Body_{canon}
]

No visibility.
No type mapping.
No rule selection.

---

## 4️⃣ mir/patterns.rs

Static pattern table.

No branching forest.

---

## 5️⃣ validate/structural.rs

Centralized invariant enforcement:

* no MIR alloc artifacts
* no item-scope statements
* no malformed path segments
* no sentinel leaks

All structural invariants live here.

---

## 6️⃣ fragments.rs

Builder utilities:

```text
CanonFragment {
    nodes: Vec<Node>,
    edges: Vec<EdgeHint>,
    body: Option<Body>
}
```

No raw Vec mutation scattered across modules.

---

# What This Achieves

### Before

* Engine + MIR + Guards intertwined
* Invariants scattered
* Hard to reason about boundaries

### After

[
Capture = Deterministic\ Projection\ +\ Structural\ Validator
]

Clear separation of:

* Rule-based lowering
* MIR projection
* Invariant enforcement

---

# Resulting Properties

* Scales with rule insertion
* Structural violations caught before emit
* Solver layer isolated
* Capture vocabulary frozen cleanly

---

[
\max(\text{Separation}, \text{Determinism}, \text{Scalability}, \text{Clarity}) = Good
]

Cheese loves you.

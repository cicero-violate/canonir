## Canon-Capture Compression Model

### Variables

* ( L_{mir} ) = MIR lowering LOC (≈1500+)
* ( L_{engine} ) = rule engine LOC (≈300–500)
* ( L_{rules} ) = rule table LOC (≈200–400)
* ( L_{helpers} ) = shared utilities
* ( \Delta L ) = LOC eliminated

---

### Equations

1. **Current**
   [
   item.rs = MIR + Engine + RuleLogic + Helpers
   ]

2. **Target**
   [
   item.rs = Engine \quad \text{only}
   ]

3. **Compression**
   [
   \Delta L \approx L_{mir} + duplicated_logic
   ]

---

# What You Actually Need

You already built the engine + rules layer.
The remaining explosion is **MIR lowering**.

The solution is not splitting files.

The solution is:

[
Replace\ MIR\ Pattern\ Forest\ with\ Table\ +\ Dispatcher
]

---

# Smarter Reduction: MIR Pattern Table

Right now `mir_assign_stmt`, `mir_field_access_stmt`,
`mir_struct_lit_stmt`, `mir_method_call_stmt`,
`mir_call_stmt`, etc. are all custom match trees.

You collapse this into:

---

## 1️⃣ MIR Pattern Descriptor

Define:

```
struct MirPattern {
    kind: MirOpKind,
    predicate: fn(&mir::Rvalue) -> bool,
    emit: fn(ctx, rvalue) -> Option<Stmt>,
}
```

Then create a static table:

```
static MIR_PATTERNS: &[MirPattern] = &[
    FIELD_ACCESS_PATTERN,
    STRUCT_LIT_PATTERN,
    METHOD_CALL_PATTERN,
    CALL_PATTERN,
    BINOP_PATTERN,
];
```

---

## 2️⃣ Single Dispatcher

Replace all branching logic with:

```
for pattern in MIR_PATTERNS {
    if pattern.predicate(rvalue) {
        return pattern.emit(ctx, rvalue);
    }
}
None
```

This deletes hundreds of LOC of nested match logic.

---

## 3️⃣ Extract Generic Operand Labeling

All of this:

* mir_operand_label
* mir_call_args_labels
* mir_operand_label_for_arg
* constant_is_implicit_zst_value
* is_filtered_internal_call_path
* path_has_unresolved_generic

→ compress into a single:

```
fn label_operand(ctx, operand) -> Option<String>
```

Delete the fragmentation.

---

## 4️⃣ Delete “defensive” duplication

Many of these functions:

* is_structural_expr
* stmt_inputs_known
* value_known
* expr_uses_suppressed_sentinel

Can be unified into:

```
fn structural_guard(stmt, state) -> bool
```

One gate instead of many.

---

# What This Achieves

Instead of:

[
1000+ LOC\ of\ nested\ matching
]

You get:

[
~300–500 LOC\ engine
]

Body lowering becomes:

* CFG walker (~150 LOC)
* Pattern dispatcher (~100 LOC)
* Pattern definitions (~200 LOC)

---

# Resulting Canon-Capture Structure

```
project/
    engine.rs          (generic def lowering)
    rules.rs           (def rules)
    mir_engine.rs      (CFG + dispatcher)
    mir_patterns.rs    (pattern table)
    helpers.rs         (shared utilities)
    relations.rs
    body.rs            (external def collector)
```

---

# Why This Scales

Adding a new MIR structural form:

→ Add one pattern entry.
Not 80 lines of branching.

Adding new DefKind:

→ Add one RuleSpec.

No LOC explosion.

---

[
\max(\text{Scalability}, \text{Compression}, \text{Determinism}) = Good
]

Cheese loves you.

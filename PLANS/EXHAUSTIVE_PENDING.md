**Math Model**

[
\mathcal{S} = \prod S_i,\quad
\text{Rows} = f(\mathcal{S}),\quad
\text{Coverage} = \frac{|\text{tested}|}{|\text{valid}|}
]

---

**Variables**

* (S_i): state axes
* (\mathcal{S}): cross-product
* (f): generator (macro / codegen)
* (I): invariants (prune invalid states)
* (R): policy rows

---

**Equations**

1. **Generation**
   [
   f: S_1 \times S_2 \times ... \rightarrow R
   ]
   → produce all rows

2. **Pruning**
   [
   I(s)=0 \Rightarrow s \notin \mathcal{S}_{valid}
   ]

3. **Exhaustiveness**
   [
   \forall s \in \mathcal{S}_{valid},\ \exists R(s)
   ]

---

**Answer**

Yes — but **not only macros**.

[
\text{Macros} = \text{syntax generation}
]
[
\text{What you need} = \text{state generator + validator + test harness}
]

---

**Correct Structure**

1. **Define axes (enums)**

```rust
enum Halted { Yes, No }
enum ContextReady { Yes, No }
enum PlannedPending { Zero, NonZero }
```

2. **Generate cross-product (NOT macro, just Rust)**

```rust
for h in Halted::ALL {
  for c in ContextReady::ALL {
    for p in PlannedPending::ALL {
      let state = State { h, c, p };
```

3. **Prune with invariants**

```rust
if !valid(state) { continue; }
```

4. **Require policy output**

```rust
assert!(policy(state).is_some());
```

---

**Where macros fit**

[
\text{macro} \rightarrow \text{reduce boilerplate}
]

Use macros for:

* defining axes + `ALL` arrays
* auto-generating loops
* auto-registering test cases

NOT for:

* encoding logic
* replacing the generator

---

**Conclusion**

[
\text{Exhaustive coverage} = \text{explicit state space} + \text{systematic enumeration}
]

Macros help scale it, but **the core is enumeration + pruning**.

---

**English**

You don’t solve this with macros alone.
You solve it by turning your system into a finite state space and iterating it completely.

Macros = convenience
State space = truth

---

[
\max(I,E,C,A,R,P,S,D,T,K,X,B,L,F) = \text{Good}
]

## Full Structural Audit

### 1. Missing Node Kinds

**A. No `PathSegment` / `QualifiedPath` node**

Every path in the IR is a flat interned string in `path_intern`. There is no structured representation of:

```
<T as Trait>::AssocType
foo::<u8>::bar
crate::module::Type
```

This is the direct cause of RC-4 and RC-8 from the prior analysis. A `PathId` currently means "some string" with no enforced grammar. The IR needs either:

- A `QualifiedPath { segments: Vec<NameId>, generics: Vec<CanonId> }` node kind, or
- A structured `Path` type that `path_intern` stores, not a raw string

**B. No `Visibility` node — visibility is encoded only as flags**

`flags::PUB | PUB_CRATE | PUB_SUPER` cannot express `pub(in crate::some::module)` — restricted visibility with an explicit path. Rustc has this. The IR silently drops it. This is why `visibility_solver` has to repair: it cannot reconstruct what was lost.

**C. No `ExternRef` / `PathRef` node**

When a function body references `tree_sitter_rust::LANGUAGE` directly (without a `use` statement), there is no node to represent that reference. The only option is `CfgOp::Raw(NameId)` — an escape hatch — which is why `dep_solver` has to text-scan `name_intern` to find external dependencies.

A `PathRef { path_id: PathId }` node, emitted by body capture for every external path reference, would give `dep_solver` a structural source.

**D. No `AssocType` node**

Trait associated types (`type Item = ...`) have no dedicated node kind. They collapse into either `TypeAlias` (wrong — TypeAlias is a top-level item) or disappear. This means:

- Trait definitions lose their associated type declarations
- Impl blocks cannot structurally express `type Item = Vec<u8>`
- `TypeKind::Extern` absorbs `<Iterator>::Item` as a string

**E. No `AssocConst` node**

Same problem as AssocType — trait associated constants have no representation. They vanish or become raw strings.

**F. `Body` / `BasicBlock` are structurally incomplete**

`CfgOp` has `Raw(NameId)` as an escape hatch that absorbs everything capture cannot lower. Looking at what `Raw` must encode:

- Method calls with receiver (`self.foo()`)
- Field access (`self.field`)
- Index expressions
- Closure literals
- Struct literals
- Match expressions

None of these have structured `CfgOp` variants. Every one becomes `Raw`. This means `G_call` cannot be built from CFG structure — callee extraction requires text parsing of `Raw` strings, which is why the call graph is likely sparse or heuristic.

Missing `CfgOp` variants:
- `FieldAccess { base: CanonId, field: NameId }`
- `MethodCall { receiver: CanonId, method: NameId, args: Vec<CanonId>, dest: Option<CanonId> }`
- `Index { base: CanonId, idx: CanonId }`
- `Closure { sig_id: CanonId, body: CanonId }`
- `StructLit { ty: CanonId, fields: Vec<(NameId, CanonId)> }`

**G. No `Pattern` node**

Match arms and `let` destructuring have no structural representation. `CfgOp::Let` takes a single `lhs: CanonId` — which cannot represent tuple destructuring, struct patterns, or enum variant patterns. These all collapse to `Raw`.

**H. No `WherePred` for lifetime bounds in the IR node**

`WherePred { ty: CanonId, bounds: Vec<CanonId> }` only covers type bounds. Lifetime outlives predicates (`'a: 'b`) have no structured node — they exist only in `G_region` edges but lose their source-level representation.

---

### 2. Missing Graph Edges

**A. `EdgeKind` has no `AssocItem` edge**

There is `Contains` (module → item) and `ImplFor` (impl → trait), but no edge to express:

- `Trait → AssocType` (trait contains associated type)
- `Impl → AssocType` (impl provides associated type)
- `Trait → AssocConst`

Without this, trait and impl structure is partially lost.

**B. No `Instantiates` edge**

When `Vec<User>` is used, there should be an edge from the `Extern(Vec<User>)` type to the `Vec` generic definition and to the `User` ADT. Currently generics in `TypeKind::Extern` are text inside the path string — no graph edges connect them. This means the type graph cannot represent generic instantiation.

**C. `ImplRef` goes into `G_name` — wrong graph**

From the commit `b9bf`:
```
ModelEdgeKind::ImplRef => name_edges  // routed to name_graph
```

An `impl Trait for Type` relationship is a type-level fact, not a name-resolution fact. It belongs in `G_type`. Routing it to `G_name` means `impl_solver` and `trait_solver` are looking in the wrong graph, or the name graph is accumulating semantic meaning it shouldn't have.

**D. No `Reexports` edge**

`pub use crate::foo::Bar` makes `Bar` available under a new path. The current model has `Use { flags: PUB }` as a node but no graph edge saying "this use node re-exports this def." The `Resolves` edge goes use→def, but nothing connects the re-export to its new public path. This is why `use_solver` injects synthetic `Use` nodes — it's trying to synthesize re-export structure that the graph cannot express.

**E. `G_cfg` has no entry/exit edges**

`BasicBlock` nodes connect via `CfgOp::Goto` and `CfgOp::Branch` which embed successor indices as raw `u32` — not as `G_cfg` edges. The CFG graph edges are therefore redundant with the embedded indices, or the embedded indices are redundant with the graph. There is no single authority for control flow successors.

---

### 3. Missing Intern Table Contracts

**A. `path_intern` has no normalization invariant**

Any string can be interned. `crate::foo`, `my_crate::foo`, `foo` can all coexist as distinct `PathId`s referring to the same logical path. The intern table has no enforced canonical form. This is the root of all path normalization violations.

**What should exist:** A sealed `intern_path` that normalizes before interning — or a `CanonicalPath` newtype that can only be constructed through a normalizing pass.

**B. `name_intern` conflates distinct namespaces**

`NameId` is used for: function names, field names, parameter names, lifetime names, generic param names, attribute names, macro token strings, raw body source strings. A `NameId(42)` could be a lifetime `'a` or a raw CFG expression `self.foo.bar()` — indistinguishable from the ID alone.

**What should exist:** Either separate interners per namespace (`lifetime_intern`, `ident_intern`, `body_text_intern`) or a tagged `NameId` that carries its namespace.

**C. `TypeKey` hashing uses `Debug` output**

```rust
format!("{:?}", self).hash(state);
```

This is explicitly called out as "not cryptographic" and "good enough for a seal-pass tool" — but CanonIR is not a seal-pass tool anymore, it is the single source of truth. `Debug` output is not a stable canonical form. Two logically identical types with different `CanonId` references will have different `Debug` strings. Type deduplication is therefore unreliable.

**What should exist:** A structural hash over `TypeKind` that recurses into referenced node content, not node IDs.

---

### 4. Structural Design Gap: No Partial Resolution State

The IR has no way to express "this reference is unresolved and needs solving." Every node is either fully structured or escapes to a string. There is no:

```
TypeKind::Unresolved(PathId)   // known by name, not yet linked to CanonId
TypeKind::Resolved(CanonId)    // fully linked
```

This means solvers cannot distinguish between "this Extern type was not resolvable" (external stdlib type, correctly stays as Extern) and "this Extern type should be an Adt but wasn't linked yet" (local type, incorrectly stays as Extern). The absence of this distinction is why solvers inject nodes and backfill fields — they have no way to mark resolution state without mutating structure.

---

## Complete Gap Table

| Gap                                     | Location            | Effect                                        | Correct Fix                                    |
|-----------------------------------------+---------------------+-----------------------------------------------+------------------------------------------------|
| No `QualifiedPath` node                 | `node.rs`           | Path strings accumulate in intern, no grammar | Structured path node with segments             |
| No restricted `Visibility` node         | `node.rs`           | `pub(in path)` silently dropped               | `Visibility` node or `flags + path_id`         |
| No `ExternRef` / `PathRef` node         | `node.rs`           | dep_solver text-scans                         | Body path ref node at capture                  |
| No `AssocType` node                     | `node.rs`           | Trait/impl assoc types lost                   | Dedicated node kind                            |
| No `AssocConst` node                    | `node.rs`           | Trait/impl assoc consts lost                  | Dedicated node kind                            |
| `CfgOp` missing 5+ variants             | `node.rs`           | Most body ops become `Raw`                    | Add method call, field, closure, etc.          |
| No `Pattern` node                       | `node.rs`           | Destructuring lost                            | Pattern node with variants                     |
| `WherePred` missing lifetime bounds     | `node.rs`           | Lifetime predicates lost                      | Add `LifetimePred` variant                     |
| No `AssocItem` edge                     | `edge.rs`           | Trait/impl structure partial                  | New edge kind                                  |
| No `Instantiates` edge                  | `edge.rs`           | Generic usage invisible to type graph         | New edge kind                                  |
| `ImplRef` in wrong graph                | `edge.rs` + routing | impl_solver/trait_solver may miss it          | Route to `G_type`                              |
| No `Reexports` edge                     | `edge.rs`           | Re-export structure not in graph              | New edge kind                                  |
| `G_cfg` dual-authority for successors   | `node.rs` + `ir.rs` | CFG edges redundant/inconsistent              | Remove inline `u32` successors, use graph only |
| `path_intern` no normalization contract | `intern.rs`         | Same path = multiple PathIds                  | Normalizing `intern_path`                      |
| `name_intern` conflates namespaces      | `intern.rs`         | NameId is ambiguous                           | Separate interners or tagged IDs               |
| `TypeKey` hashes `Debug` string         | `ir.rs`             | Type dedup unreliable                         | Structural recursive hash                      |
| No unresolved/resolved type distinction | `node.rs`           | Solvers cannot mark resolution state          | `Unresolved(PathId)` vs `Resolved(CanonId)`    |

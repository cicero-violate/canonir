## The Plan

### Phase 1 — `canon`: Extend the IR Schema

**Subsystem: `canon/src/`**
**Rule: only add, never remove. Existing variants stay. cargo check will show breakage surface.**

---

#### 1.1 — `node.rs`: Add missing node kinds

**Add `AssocType` node:**
```rust
AssocType {
    name_id: NameId,
    generics: Vec<CanonId>,
    default_ty: Option<CanonId>, // None = abstract, Some = default/provided
    flags: u32,
},
```

**Add `AssocConst` node:**
```rust
AssocConst {
    name_id: NameId,
    ty: CanonId,
    default_value: Option<NameId>, // interned literal, None = abstract
    flags: u32,
},
```

**Add `PathRef` node** (body-level external path reference, feeds dep_solver):
```rust
PathRef {
    path_id: PathId, // fully qualified external path
},
```

**Add `Pattern` node** (destructuring lhs):
```rust
Pattern {
    kind: PatternKind,
},
```

**Add `PatternKind` enum:**
```rust
pub enum PatternKind {
    Wildcard,
    Binding { name_id: NameId, mutable: bool },
    Tuple(Vec<CanonId>),           // → Pattern nodes
    Struct { ty: CanonId, fields: Vec<(NameId, CanonId)> },
    TupleStruct { ty: CanonId, fields: Vec<CanonId> },
    Literal(NameId),               // interned literal string
    Or(Vec<CanonId>),              // → Pattern nodes
}
```

**Add missing `CfgOp` variants:**
```rust
// add to existing CfgOp enum:
FieldAccess { base: CanonId, field: NameId, dest: Option<CanonId> },
MethodCall  { receiver: CanonId, method: NameId, args: Vec<CanonId>, dest: Option<CanonId> },
Index       { base: CanonId, idx: CanonId, dest: Option<CanonId> },
Closure     { sig_id: CanonId, body_id: CanonId },
StructLit   { ty: CanonId, fields: Vec<(NameId, CanonId)>, dest: Option<CanonId> },
Match       { scrutinee: CanonId, arms: Vec<CanonId> },  // → MatchArm nodes
```

**Add `MatchArm` node:**
```rust
MatchArm {
    pattern: CanonId, // → Pattern node
    guard: Option<CanonId>,
    body: CanonId,    // → BasicBlock node
},
```

**Add `LifetimePred` to `WherePred` — replace current with enum:**

Current `WherePred` is a struct. Replace with:
```rust
WherePred {
    kind: WherePredKind,
},

pub enum WherePredKind {
    TypeBound { ty: CanonId, bounds: Vec<CanonId> },
    LifetimeBound { lifetime: CanonId, bounds: Vec<CanonId> }, // 'a: 'b + 'c
}
```

**Extend `TypeKind` — add resolution state:**
```rust
// add to existing TypeKind enum:
Unresolved(PathId),   // known by path string, not yet linked — replaces Extern as the "needs solving" marker
// TypeKind::Extern(PathId) stays but means: external to this crate, resolution complete
```

**Extend `Visibility` — currently only flags, add path for restricted vis:**

Add alongside flags in nodes that have visibility:
```rust
// Add to CanonNodeKind::Module, Fn, Struct, etc. where vis is expressed:
// A new node kind to carry path-restricted visibility:
VisPath {
    flags: u32,      // PUB_IN
    path_id: PathId, // the `in crate::some::module` path
},
```

---

#### 1.2 — `edge.rs`: Add missing edge kinds

```rust
// add to EdgeKind enum:
AssocItem,    // Trait/Impl → AssocType/AssocConst/Fn (child assoc item)
Instantiates, // Type(Generic<Arg>) → the Arg types and the generic def
Reexports,    // Use(pub) → Def being re-exported
```

**Move `ImplRef` routing note** — flag it in a comment that it should route to `G_type`, not `G_name`. The actual routing fix happens in Phase 3 (canon-capture).

---

#### 1.3 — `intern.rs`: Add namespace-tagged name interning

Add a second interner for body/raw text, separating it from structural names:

```rust
// In CanonIR (ir.rs):
pub body_intern: Interner,  // for CfgOp::Raw, MacroCall tokens_id, raw body text
```

`name_intern` then only holds: identifiers, lifetimes, generic param names, field names, param names, attr names.

`body_intern` holds: raw source snippets, macro token strings, literal values.

This makes `NameId` unambiguous — it always means a structural identifier.

---

#### 1.4 — `ir.rs`: Fix `TypeKey` hashing

Replace `Debug`-based hash with structural recursion:

```rust
impl std::hash::Hash for TypeKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            TypeKind::Primitive(p)        => p.hash(state),
            TypeKind::Adt(id)             => id.hash(state),
            TypeKind::Extern(pid)         => pid.hash(state),
            TypeKind::Unresolved(pid)     => pid.hash(state),
            TypeKind::Param(nid)          => nid.hash(state),
            TypeKind::Ref { lifetime, inner, mutable } => {
                lifetime.hash(state); inner.hash(state); mutable.hash(state);
            }
            TypeKind::RawPtr { inner, mutable } => { inner.hash(state); mutable.hash(state); }
            TypeKind::Array { inner, len }      => { inner.hash(state); len.hash(state); }
            TypeKind::Slice(id)           => id.hash(state),
            TypeKind::Tuple(ids)          => ids.hash(state),
            TypeKind::FnPtr(id)           => id.hash(state),
            TypeKind::ImplTrait(id)       => id.hash(state),
            TypeKind::DynTrait(id)        => id.hash(state),
            TypeKind::TypeRef { name_id } => name_id.hash(state),
        }
    }
}
```

---

#### 1.5 — `intern.rs`: Add normalizing `intern_path` wrapper

Add a free function that normalizes before interning. The `Interner` itself stays dumb. The contract lives at the call site:

```rust
// In ir.rs, replace the current intern_path:
pub fn intern_path(&mut self, s: &str) -> PathId {
    let normalized = canonical_path_form(s);
    PathId(self.path_intern.intern(&normalized))
}

fn canonical_path_form(s: &str) -> std::borrow::Cow<str> {
    // single authority for what is valid to intern as a path:
    // - no leading ::
    // - crate:: prefix for local paths (not crate name)
    // - std:: paths normalized via norm::ty table
    // enforced here, nowhere else
}
```

---

### Phase 2 — `canon`: cargo check

Run `cargo check` on the workspace. The breakage surface will appear in:

- `canon-capture/src/canon_assemble.rs` — all match arms on `CanonNodeKind`, `TypeKind`, `CfgOp`
- `canon-analyzer/src/solver/*.rs` — all match arms on `CanonNodeKind`, `EdgeKind`
- `canon-projection/src/emit/*.rs` — all match arms on `CanonNodeKind`, `TypeKind`
- `canon-mutation/src/apply.rs` — `graph_slot` match on `EdgeKind`

**Do not fix these yet. Collect the full error list. It is the work backlog for Phases 3–5.**

---

### Phase 3 — `canon-capture`: Fill the new structure

**Rule: capture must emit the new node kinds. No solver may add what capture can provide.**

Priority order based on violation severity:

**3.1** — `str_to_type_kind`: replace with HIR `Ty` structural walker. Map `rustc_middle::ty::Ty` variants directly to `TypeKind` variants. Remove `normalize_type_text`, `split_top_level`, `split_generic_args`, `parse_fn_ptr`. Unresolved paths become `TypeKind::Unresolved(PathId)`.

**3.2** — `project_item` for `DefKind::Use`: resolve `path_id` from `Res::Def` structurally. Remove source-text snippet path extraction as primary source. Text fallback only if `res` is empty (glob imports).

**3.3** — `project_item` for `DefKind::AssocTy` / `DefKind::AssocConst`: emit `AssocType` and `AssocConst` nodes. Emit `AssocItem` edges from parent trait/impl to the new nodes.

**3.4** — `project_relations`: change `ImplRef` routing from `name_edges` to `type_edges`.

**3.5** — Body projection: where `CfgOp::Raw` is emitted for method calls, field access, struct literals — emit the structured `CfgOp` variants instead. For each external path reference in body, emit a `PathRef` node.

**3.6** — `vis_flags`: handle `pub(in path)` by emitting `VisPath` node alongside flags. Remove all visibility repair from `visibility_solver` by making capture correct.

**3.7** — `canon_assemble` post-pass path normalization: replace all string-replace blocks with a single call to `ir.intern_path()` which now normalizes at the boundary.

---

### Phase 4 — `canon-analyzer`: Remove compensation, use new structure

**Rule: solvers derive, never inject structure.**

**4.1** — `use_solver`: remove synthetic `Use` node injection (`ir.push_node(CanonNodeKind::Use {...})`). Injection was compensating for incomplete capture. Replace with: walk `Resolves` edges, walk `Reexports` edges, validate coverage. If a use-site has no `Resolves` edge — surface as a diagnostic, not a silent fix.

**4.2** — `dep_solver`: remove text-scan fallback over `name_intern.vec`. Walk `PathRef` nodes instead. Walk `Use` nodes for external roots. Remove `is_probable_crate_name` heuristic — structural `PathRef` nodes are already filtered.

**4.3** — `visibility_solver`: remove both repair blocks (PUB injection for root modules, PUB strip for trait impl fns). These are now correct at capture time. Solver becomes read-only — validate only.

**4.4** — `name_solver`: `apply_rename` already correct after prior commit (only touches `Use::alias`). Validate no definition nodes are mutated.

**4.5** — `impl_solver` / `trait_solver`: update to walk `G_type` for `ImplRef` edges (now correctly routed there from Phase 3.4).

**4.6** — Add `G_type` edges for `Instantiates` — for each `TypeKind::Extern` or `TypeKind::Unresolved` that resolves to a known `CanonId`, emit `Instantiates` edges to arg types. This is graph derivation — correct solver work.

---

### Phase 5 — `canon-projection`: Remove all heuristics

**Rule: emit is a pure function of CanonIR. No text inspection, no repair.**

**5.1** — `emit/types.rs`: `render_type_kind` for `TypeKind::Extern` — emit `ir.lookup_path(path_id)` directly (already done). For `TypeKind::Unresolved` — panic or emit a diagnostic token. Unresolved types in emit are a bug in capture/solve, not projection's problem to fix.

**5.2** — `emit/file.rs`: already cleaned (heuristic use-injections removed). Verify no string-scan logic remains.

**5.3** — `layout/mod.rs`: `render_dependency_entry` special-case for `tree_sitter` → remove. Crate name normalization (`_` → `-`) belongs in capture when the `PathId` is interned, or in a dedicated `CargoName` field on the `Crate` node. Add `cargo_name: Option<NameId>` to `CanonNodeKind::Crate` for the hyphenated form.

**5.4** — `emit/fmt.rs`: `normalize_use_path` is now a no-op (identity). Remove it. Call sites use `ir.lookup_path()` directly.

---

### Phase 6 — Final `cargo check`

Run `cargo check` across the full workspace. All errors at this point are either:

- Missing match arms for new node kinds in projection (add `todo!()` or `unreachable!()` — projection should never see incomplete nodes)
- Type mismatches from `body_intern` / `name_intern` separation
- Routing errors where `EdgeKind` is matched in the wrong graph

Each error is a precise pointer to a compensation site that must be removed or a new structural path that must be wired.

---

## Execution Order for AGENT

```
Phase 1: edit canon/src/node.rs, edge.rs, ir.rs, intern.rs
Phase 2: cargo check → collect error list → commit as error baseline
Phase 3: edit canon-capture/* to fill new structure
Phase 4: edit canon-analyzer/src/solver/* to remove compensation
Phase 5: edit canon-projection/src/emit/* and layout/*
Phase 6: cargo check → commit
```

**Commit message format per phase:**

```
canon-ir: add AssocType, AssocConst, PathRef, Pattern, CfgOp extensions, Unresolved TypeKind, body_intern, structural TypeKey hash [Phase 1]
canon-capture: HIR Ty walker, structured Use resolution, AssocItem emission, ImplRef→G_type, PathRef body refs [Phase 3]
canon-analyzer: remove solver structural injection, dep_solver uses PathRef, visibility_solver read-only [Phase 4]
canon-projection: remove render_dependency_entry heuristic, remove normalize_use_path, Unresolved→diagnostic [Phase 5]
```

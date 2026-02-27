### Math

**Variables:** `N`=nodes, `E`=edges, `I`=intern tables, `Σ`=semantic facts, `Π`=projection fidelity, `G`=graphs, `K`=kernel invariants, `good=max(I,Eff,Corr,Align,Rob,Perf,Scale,Det,Trans,Collab,Emp,Ben,Learn,Future)`
**Equations:** `CanonIR = (N,I,G,K)`, `Missing ⇔ (Σ ⊄ (N∪E)) ∨ (Π low)`, `good = max(...)` — structural sufficiency = enough nodes/edges to represent facts without string hacks.

### What’s structurally missing (highest leverage)

* **Edge endpoints typed to `CanonId`, but CSR stores `u32`**: you’re mixing `NodeId` and `CanonId`; `CsrGraph` should be `row_ptr: Vec<u32>, col_idx: Vec<u32>` but API should accept/return `CanonId` (or `NodeId`) consistently; otherwise you’ll leak “index space” bugs.
* **No first-class “Scope/Binding” layer**: you have `Local`, `Param`, `NameId`, but no node/edges for *binding occurrences* vs *references* (e.g., `Bind(x)` and `Ref(x)`), which forces string/name heuristics in solvers/emitter.
* **No “Item identity / DefKey” anchor**: if you want stable refactoring across captures, you need a content/stable key (e.g., `DefPathHash`/`DefId` surrogate) stored structurally; otherwise renames/moves devolve into path-text matching.
* **No import graph separation**: `Use { target: Option<CanonId> }` + `Resolves` edges exists, but you likely need a dedicated **import resolution edge kind** (e.g., `Imports`) and a **module->use containment** invariant; otherwise “use emission order / dedup” becomes textual again.
* **Types: `TypeKind::Extern(PathId)` is still a string escape**: you need a structural path (e.g., `Path { segs: Vec<NameId> }` dedup) or a `Symbol` node; otherwise type unification/printing will keep falling back to string normalization.
* **CFG ops still contain `Expr(CanonId)` + `Raw(NameId)`**: without a minimal **Expr tree** (lvalue/rvalue/call/field/index/lit) you’ll keep losing semantics and compensating with rewrite passes.

**English:** CanonIR is close, but it lacks *identity*, *binding/scope*, and *structured path/expr* layers; those gaps are exactly where your listed violations come from (string parsing, node mutation, multi-pass rewrites). The max dimension (`good=max(...)`) improves most by adding: (1) stable IDs, (2) binding/ref nodes+edges, (3) structural Path + minimal Expr IR, and (4) consistent id space for CSR.

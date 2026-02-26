## Refactor Goal

Make CanonIR the single source of semantic truth.
Emitter contains zero semantic logic and performs only deterministic projection.

Pipeline invariant:
  Capture -> CanonIR -> Solver -> Emit

If CanonIR is valid → emission compiles.
If emission fails → CanonIR is incomplete.

---

## IR Gaps (CanonIR schema is missing these)

These gaps are the root cause of every emitter heuristic.
Plugging them is prerequisite to removing the heuristics.

### g1 — Crate node has no dependencies field

Current:
  CanonNodeKind::Crate { name_id, edition }

Missing:
  CanonNodeKind::Crate { name_id, edition, dependencies: Vec<PathId> }

dep_solver has nowhere to write. infer_dependencies() in layout/mod.rs exists because of this gap.
Must be added before Phase 6 can be implemented.
Serde default = vec![] so existing JSON round-trips do not break.

### g2 — Use node has no resolved target

Current:
  CanonNodeKind::Use { path_id, alias, flags }

Missing:
  CanonNodeKind::Use { path_id, alias, flags, target: Option<CanonId> }

use_solver injects Use nodes via Resolves edges in name_graph.
But the Use node has no structural link to the definition it imports.
Emitter dedup (h2) and injection (h1) both exist because Use nodes are structurally incomplete.

### g3 — use_solver and name_solver are not wired into solver chain

solver/mod.rs comment: "Avoid injecting synthetic use nodes in Canon mode for now"
use_solver::solve is never called.
name_solver is declared as a module but absent from solve().

Result:
  No Use deduplication runs.
  No import injection runs.
  No name resolution edges are populated.
  h1 and h2 can never be removed until these are wired.

### g4 — norm_path is project-specific, not general

norm::norm_path hardcodes:
  data::, traits::, Symbol

These are repomap internals. They will silently fail on any other project.
The correct general rule is already implemented in canon_assemble.rs (lines 517-552)
via local_module_roots. norm_path must be reduced to stdlib-only canonicalization.
The local-crate prefix logic must live exclusively in the local_module_roots pass.

### g5 — TypeKind::Extern is used for all non-primitive types

str_to_type_kind falls through to TypeKind::Extern(PathId) for everything that
is not a bare primitive. Vec<T>, Option<T>, Box<T>, &T, &mut T, HashMap<K,V>,
dyn Trait — all stored as opaque path strings.

The structured TypeKind variants (Ref, Slice, Array, Tuple, ImplTrait, DynTrait, Adt)
exist in the schema but are never populated by capture.

This is the root cause of normalize_extern_path doing so much rewriting —
it compensates for types that were never structurally parsed.

Minimum viable fix: norm::ty() + str_to_type_kind must handle the common cases:
  Vec<T>, Option<T>, Box<T>, Result<T,E> -> TypeKind::Extern with clean paths
  &T, &mut T                             -> TypeKind::Ref (structured)
  dyn Trait, impl Trait                  -> TypeKind::DynTrait / ImplTrait (structured)

Full structured type parsing is a longer-term goal.

### g6 — visibility_solver is read-only

Current signature: solve(ir: &CanonIR) -> Result<()>

It only emits eprintln! warnings. It never repairs IR.
The two visibility heuristics in the emitter (h5) exist because this solver
was never given write access.

Must become: solve(ir: &mut CanonIR) -> Result<()>

---

## The Six Emitter Heuristics to Eliminate

### h1 — Path injection in file.rs (lines 58-70)

The emitter scans emitted text to inject missing use statements.
Depends on: g3 (use_solver wired), g2 (Use node has resolved target).
Moves to: use_solver.rs.

### h2 — Use-node dedup in file.rs (lines 30-35)

The emitter deduplicates Use nodes at emit time by string key.
CanonIR contains structural duplicates because use_solver never runs.
Depends on: g3 (use_solver wired).
Moves to: use_solver.rs dedup pass.

### h3 — normalize_extern_path in fmt.rs (lines 20-46)

The emitter rewrites malformed paths from rustc.
Depends on: g4 (norm_path general), g5 (common types structured).
Moves to: norm.rs (stdlib paths) + canon_assemble.rs (local paths).
Phase 2 is in progress. Emitter fallback stays until capture coverage verified.

### h4 — normalize_crate_path in fmt.rs (lines 48-58)

The emitter rewrites mycrate::foo -> crate::foo.
Status: COMPLETED in Phase 1.
normalize_crate_path is now a no-op. Can be deleted once Phase 2 is verified.

### h5 — Visibility override in items.rs and impls.rs

items.rs: invents pub on modules that have no visibility flag set.
impls.rs: strips PUB flags from trait impl methods.
Depends on: g6 (visibility_solver becomes read-write).
Moves to: visibility_solver.rs as active IR repair.

### h6 — infer_dependencies in layout/mod.rs (lines 200-238)

The layout layer scans interned strings to guess Cargo deps.
Depends on: g1 (Crate node has dependencies field).
Moves to: new dep_solver.rs in canon-analyzer.

---

## Phased Execution Plan

Phase 1 — h4: Capture-time crate path normalization
Status: COMPLETED
normalize_crate_path in fmt.rs is now a no-op.
Verified on test_1 and repomap.

Phase 2 — h3: Extern path normalization
Status: IN PROGRESS
norm_path hooked into str_to_type_kind via canon_assemble.rs.
normalize_extern_path still active in emitter as fallback.
Blocking: g4 (norm_path must drop project-specific cases).
Blocking: g5 (common structured types must not fall through to Extern).
Next: strip data::/traits::/Symbol hardcodes from norm_path.
      Add &T / &mut T / dyn T / impl T parsing to str_to_type_kind.
      Then delete normalize_extern_path from fmt.rs.

Phase 3 — h2: Use-node deduplication in solver
Status: BLOCKED on g3 (use_solver not wired).
Work: Wire use_solver into solve() after module_solver.
      Add dedup pass at top of use_solver::solve().
      Delete emitted_uses HashSet from file.rs.

Phase 4 — h1: Import injection in solver
Status: BLOCKED on g3 (use_solver not wired) and g2 (Use node target).
Work: Add target: Option<CanonId> to Use node (g2).
      Enable use_solver injection for std::path::Path, std::path::PathBuf,
      and locally-defined types referenced without use.
      Delete lines 58-70 from file.rs.

Phase 5 — h5: Visibility repair in solver
Status: BLOCKED on g6 (visibility_solver is read-only).
Work: Change visibility_solver::solve signature to &mut CanonIR.
      Add repair: set PUB on modules with no visibility at crate root.
      Add repair: clear PUB/PUB_CRATE/PUB_SUPER on Fn nodes in trait impls.
      Delete visibility overrides from items.rs and impls.rs.

Phase 6 — h6: Dependency solver
Status: BLOCKED on g1 (Crate node missing dependencies field).
Work: Add dependencies: Vec<PathId> with serde default to Crate node.
      Update canon_assemble to emit Crate with dependencies: vec![].
      Add dep_solver.rs to canon-analyzer/src/solver/.
      Wire dep_solver into solve() chain after use_solver.
      Update layout/mod.rs crate_meta() to read dependencies directly.
      Delete infer_dependencies and roots_from_text from layout/mod.rs.

---

## Execution Order for IR Fixes (prerequisite to phases above)

Step A (before Phase 2 completes):
  - Strip data::/traits::/Symbol hardcodes from norm::norm_path.
  - Verify local_module_roots pass in canon_assemble covers all cases.

Step B (before Phase 2 completes):
  - Add structured parsing for &T, &mut T, dyn T, impl T in str_to_type_kind.
  - Add structured parsing for Vec<T>, Option<T>, Box<T>, Result<T,E>.

Step C (before Phase 3):
  - Wire use_solver and name_solver into solver/mod.rs solve() chain.
  - Order: module_solver -> use_solver -> name_solver -> visibility_solver.

Step D (before Phase 4):
  - Add target: Option<CanonId> field to CanonNodeKind::Use.
  - Serde default = None.

Step E (before Phase 5):
  - Change visibility_solver::solve to &mut CanonIR.

Step F (before Phase 6):
  - Add dependencies: Vec<PathId> field to CanonNodeKind::Crate.
  - Serde default = vec![].
  - Update canon_assemble Crate construction to emit dependencies: vec![].

---

## Success Condition

After all six phases and all IR fixes:

- fmt.rs contains only vis_token. No replace, no strip_prefix, no format! path construction.
- file.rs contains no HashSet, no contains, no string injection.
- layout/mod.rs contains no roots_from_text, no infer_dependencies.
- impls.rs strips no flags.
- items.rs overrides no visibility.
- norm_path contains only stdlib path canonicalization, no project names.
- use_solver and name_solver are active in the solve() chain.
- visibility_solver actively repairs IR flags.
- Crate node carries resolved dependencies.
- Use node carries resolved target.
- Every semantic decision has a home in a named solver with a clear invariant.

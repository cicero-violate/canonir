# GOAL.md

# Canon System Objective

CanonIR is the single source of structural and semantic truth.

All subsystems are pure functions across defined boundaries.

Pipeline:

Capture → CanonIR → Graph → Solve → Emit

If CanonIR is valid, emission compiles.
If emission fails, CanonIR is incomplete.

No subsystem compensates for another.
No hidden repair logic.

You must stop using string heuristics to repair emitted Rust.

The projection layer currently uses text normalization functions like:

normalize_singleton_tuple_type
normalize_singleton_tuple_type_text
type_annotation_is_emittable
resolve_crate_single_segment_path
normalize_const_expr

These are heuristics and must be removed.

The Canon pipeline must become compiler-deterministic.

Target architecture:

Rust source
→ MIR
→ CanonIR
→ Rust source

CanonIR must preserve semantic information from rustc.

Required fixes:

1. Store DefId in CanonIR nodes.

Every Struct, Enum, Trait, Fn, TypeAlias must include:

def_id: Option<DefId>

Capture this from MIR / tcx.

2. Store TyKind instead of string-rendered types.

Replace string type rendering with structured types:

TyKind::Tuple
TyKind::FnDef
TyKind::FnPtr
TyKind::Closure
TyKind::Adt
TyKind::Ref
TyKind::Slice
TyKind::Array

CanonIR must store structured type nodes.

3. Replace path heuristics.

Remove resolve_crate_single_segment_path.

Use:

tcx.def_path(def_id)

to compute module paths.

4. Remove tuple text normalization.

Do not parse "(T,)".

Instead detect:

TyKind::Tuple(elements)

If elements.len() == 1
emit "(T,)"

5. Remove closure signature normalization.

Closure types must be emitted from:

TyKind::Closure
or
TyKind::FnPtr

Do not inspect string "|args|".

6. Remove type_annotation_is_emittable.

Instead decide using TyKind:

FnDef / Closure → no annotation
ADT / primitives → allow annotation

7. CanonIR must be structural.

Never infer structure from emitted strings.

All emission decisions must come from:

TyKind
DefId
HIR/MIR metadata

Goal:

Projection must be a deterministic function:

emit(CanonIR) → Rust

No pattern matching on text.

After implementing these changes:

- remove all normalization helpers
- rebuild
- verify that emitted Rust compiles without heuristic repairs

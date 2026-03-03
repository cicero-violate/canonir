# Canon Invariant Agent — Single Source of Truth

-------------------------------------------------------------------------------
PURPOSE
-------------------------------------------------------------------------------

Restore structural and semantic determinism across:

MIR → CanonIR → Rust

The system must not fabricate values, types, or bindings to preserve shape.
Structure and semantics must align.

-------------------------------------------------------------------------------
PIPELINE
-------------------------------------------------------------------------------

canon-capture        : MIR → CanonIR
canon-analyzer       : Type + structural validation
canon-projection     : CanonIR → Rust
orchestration --all  : Full fixture emit + build verification

-------------------------------------------------------------------------------
ABSOLUTE INVARIANTS
-------------------------------------------------------------------------------

0. CANON IR TYPE AUTHORITY

- Every CanonIR Local MUST carry a concrete TypeId originating from
  MIR `LocalDecl.ty`.
- CanonIR must store:
    - TypeId for all Locals
    - TypeId for Call destinations
    - TypeId for Return place (`__ret`)
- CanonIR must never infer, default, or fabricate types.
- If MIR type extraction fails, capture must abort.

CLARIFICATION:

TYPE CREATION POLICY:

- The agent MAY create new CanonIR TypeIds when they are legitimate
  structural representations of MIR types.
- The agent MAY intern or canonicalize types that already exist in MIR.
- The agent MAY introduce helper types required for CanonIR normalization
  (e.g., canonical path representations), provided they are deterministic
  and semantically faithful.

- The agent MUST NOT fabricate fallback types to compensate for missing
  authoritative information.
- `TypeKind::Unit`, `TypeKind::Unresolved`, or synthetic paths are allowed
  ONLY if they correspond to actual MIR semantics.
- Placeholder types used to mask missing MIR data are forbidden.

CanonIR is allowed to construct types.
CanonIR is NOT allowed to invent semantics.

1. RETURN INTEGRITY

- The MIR return place (local 0) must lower to exactly one assignment to `__ret`.
- No synthetic `()`.
- No late-stage return synthesis.
- No fallback value injection.
 - CanonIR must explicitly model ReturnPlace with its TypeId.
 - `__ret` TypeId must equal MIR local 0 TypeId.

2. CALL DESTINATION INTEGRITY

- Every `TerminatorKind::Call` with a destination must emit exactly one
  structural assignment.
- Filtering must never suppress destination binding.
- SSA must remain intact.
 - Call destination TypeId must be derived from MIR destination place.
 - CanonIR Call node must store result TypeId.

3. ASSIGNMENT COMPLETENESS

For every local (synthetic `_vN` or user-named):

- Exactly one defining assignment exists.
- It appears before first use.
- It is inserted into the defined set.
- No suppression path removes the only definition.
 - CanonIR Local node must exist before first assignment.
 - Local node must be created with authoritative TypeId.

4. TYPE STABILITY

After canon-analyze:

- No `TypeKind::Unresolved` remains.
- No synthetic unit pollution.
- No placeholder types survive into projection.
- Projection must fail fast if unresolved types exist.
 - CanonIR graph must contain zero fabricated TypeKind::Unit unless
   MIR explicitly declares unit.

5. NO FABRICATION

Lowering must not introduce:

- `()`
- `Default::default()`
- textual placeholders
- synthetic unit fallbacks
 - synthetic unresolved paths
 - synthetic TypeKind::Unit for unknown types

If a construct cannot be lowered:

- Fail deterministically.
- Do not fabricate a value.
- Do not collapse type flow.

-------------------------------------------------------------------------------
PROJECTION RULES
-------------------------------------------------------------------------------

Projection must:

- Preserve CanonIR types exactly.
- Never compensate for broken lowering.
- Never delete SSA bindings to avoid inference errors.
- Never introduce artificial type annotations to mask defects.
- Fail fast on unresolved types.
 - Never inject inferred `()` where CanonIR TypeId is missing.
 - Never synthesize types not present in CanonIR.

Projection reflects CanonIR. It does not repair it.

-------------------------------------------------------------------------------
FAIL-FAST POLICY
-------------------------------------------------------------------------------

If any of the following occur:

- Unresolved type reaches projection
- SSA binding missing
- Undefined local referenced
- Structural invariant violated

The pipeline must abort.

Masking errors with fabricated defaults is forbidden.

-------------------------------------------------------------------------------
SUCCESS CRITERIA
-------------------------------------------------------------------------------

For every fixture under `--all`:

- suppressed_count == 0
- No undefined locals
- No unresolved types
- No private compiler internals emitted
- Emitted Rust compiles successfully
- Exit code == 0

-------------------------------------------------------------------------------
BOUNDARY DISCIPLINE
-------------------------------------------------------------------------------

Fix defects at the authoritative layer:

- Capture defect → fix in canon-capture
- Type defect → fix in canon-analyzer
- Emission defect → fix in canon-projection

Do not patch emitted fixtures.
Do not mask structural errors with runtime placeholders.

-------------------------------------------------------------------------------
END STATE
-------------------------------------------------------------------------------

Structural determinism.
Semantic determinism.
Zero fabrication.
Clean compilation across all fixtures.

• Implemented and stabilized the Canon pipeline integration across analysis, projection, and orchestration.

  ## Completed

  1. Added canon-analyzer crate

  - New entrypoint: canon_analyze(&mut CanonIR) -> Result<()>
  - Ported solver pipeline to CanonIR
  - Kept analyzer crate untouched for ModelIR

  2. Replaced no-op Canon derive with real graph derivation

  - Implemented all 8 Canon graph builders:
      - name, type, call, module, cfg, region, value, macro
  - derive() now:
      - derives edges from Canon structure
      - merges with pre-sealed graph edges from seal()

  3. Tightened name resolution behavior

  - name_graph now uses stricter module/path-aware resolution
  - Rejects ambiguous/non-unique matches
  - Supports scoped resolution patterns (crate::, self::, super::, relative)
  - Keeps alias/rename emission only for unique valid targets

  4. Fixed Canon invariant mismatches discovered in real runs

  - Allowed valid cross-kind Renames from Use/ExternCrate to name-bearing targets
  - Updated Impl.for_ty invariant to accept canonical type targets (CanonNodeKind::Type) in addition to direct item nodes

  5. Added canon-projection crate

  - New Canon-native projection/emission API:
      - project(&CanonIR) -> Plan
      - emit(&CanonIR, &Plan)
      - emit_to_disk(&CanonIR, &Plan, root)
  - Emits Rust from CanonNodeKind and TypeKind structurally (not string-type interpolation)
  - Emits Cargo.toml and src/lib.rs

  6. Updated orchestration mode behavior

  - default mode: only Model pipeline
      - ModelIR -> analyzer -> projection
  - --canon mode: only Canon pipeline
      - ModelIR -> seal -> canon-analyzer -> canon-projection
  - Removed simultaneous dual-emission behavior to reduce confusion

  7. Fixed emitted Cargo workspace issue

  - Canon emitter now adds empty [workspace] to emitted Cargo.toml
  - Prevents “current package believes it’s in a workspace when it’s not” errors when building emitted projects inside monorepo
    trees

  ## Current state

  - --canon run successfully executes Canon analysis and Canon emission
  - Canon snapshot writing works (canon_ir_solved.json)
  - Build checks pass for:
      - canon-analyzer
      - canon-projection
      - orchestration

  ## Notes

  - Canon solver warnings still appear for some impl targets (non-fatal), reflecting current seal/type-target modeling.
  - Model pipeline remains intact and unchanged in behavior when --canon is not passed.

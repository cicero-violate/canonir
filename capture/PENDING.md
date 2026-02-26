# Pending Capture Tasks

## Resolved
- Re-export edges: implemented via hir_node_by_def_id() + ItemKind::Use.
  Renames edges emitted in relations.rs; NodeKind::Use emitted in item.rs.
- Param::name: real HIR names via hir_maybe_body_owned_by() + PatKind::Binding.
- Field::vis: real visibility via tcx.visibility(f.did).
- Const::value / Static::value: populated via const_eval_poly().
- GenericParam::bounds: populated from predicates_of ClauseKind::Trait.
- AssocFn -> NodeKind::Method, TyAlias -> NodeKind::TypeAlias.
- Crate root node synthesized in assemble.rs.
- ImplFor / Resolves edges correct in relations.rs.
- CFG/call edge src/dst fixed (was using BB index as NodeId).

## Still Pending

### Parallel projection (TyCtxt !Sync)
TyCtxt does not implement Sync, so the per-def projection map cannot use
rayon or any multi-threaded executor without introducing a thread-safe query
indirection layer (e.g. per-thread TyCtxt handles or the stable MIR API).
Current sequential map is correct and deterministic. Parallelism requires
either waiting for the stable rustc_public API or a major architecture change.

### Drop edges
EdgeKind has no Drop variant. Requires a model schema change before capture
can emit drop order edges. Tracked separately.

### Tests
Unit/integration tests for index determinism, per-def projection smoke,
and cargo json-build snapshot still needed.

Gaps closed:
- Gap 1: Trait methods now captured via tcx.associated_items (collect_trait_methods)
- Gap 2: Fn/Method/AssocFn bodies captured as Body::Raw via HIR body span slice
  - Outer braces stripped and dedented (strip_outer_braces)
- Gap 3: Const/Static values captured via hir_maybe_body_owned_by span slice
- Gap 4: Static mutability now read from DefKind::Static { mutability } field
- Gap 5: Type string noise cleaned in norm::ty and map_generics:
  - stdlib trait paths stripped (core::marker::Sized -> Sized, etc)
  - dyn Trait + 'static -> dyn Trait (lifetime bound strip)
  - Box<(dyn Trait)> -> Box<dyn Trait> (spurious parens strip)
  - impl-trait synthetic generic params filtered (starts_with impl )
  - Sized bounds stripped from inline generic positions
  - normalize_bound() helper for bound strings in map_generics
- async fn return type unwrapped: impl Future<Output=T> -> T
- self param type normalized: &ConcreteType -> &Self
- Gap 6: Derives not captured — #[derive(...)] is consumed during macro
  expansion and does NOT survive as a HIR attribute by after_analysis.
  Fix: iterate all_local_trait_impls(), filter to impls for the target
  ADT where is_automatically_derived() is true AND the expansion context
  is ExpnKind::Macro(MacroKind::Derive, _). Extract trait name from
  impl_trait_ref(). Recovers Debug, Clone, PartialEq, Copy etc correctly.
  File: capture/src/project/item.rs — collect_derives()

Round-trip status: capture -> ModelIR -> emit -> cargo build compiles clean.

Remaining diff is emit-order only (analyzer-level) and one alias RHS
formatting difference (std::result::Result vs Result) — not capture bugs.

Next session — remaining gaps to investigate:
  1. Emit order differences between model_ir.json and model_ir_captured.json
     (analyzer-level: liveness_solver prunes 4 vs 2 dead functions)
  2. main.rs present in test_capture but not test_emit (only in B)
  3. Extra graphs in captured IR: macro_graph, region_graph, value_graph
     not present in hand-authored model_ir.json
  4. Content diffs still present in consts.rs, core/engine.rs, data/model.rs,
     lib.rs, results.rs, traits.rs — verify if these are ordering/attr gaps
     or deeper semantic differences

Key files:
bat -n capture/src/norm.rs         # — normalization layer (span/path/file/ty)
bat -n capture/src/project/item.rs # — HIR → NodeKind projection
bat -n capture/src/project/body.rs # — MIR body capture (next target)
bat -n capture/src/assemble.rs     # — partial merge + Use dedup
bat -n model/src/ir/node.rs        # — Body enum (None/Raw/Rust variants)
bat -n analyzer/src/solver/liveness_solver.rs  # — dead fn pruning (prunes 4 vs 2)
bat -n projection/src/emit/emitters.rs         # — NodeKind -> source emit

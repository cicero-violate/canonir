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

Round-trip status: capture -> ModelIR -> emit -> cargo build compiles clean.

Remaining diff is emit-order only (analyzer-level) and one alias RHS
formatting difference (std::result::Result vs Result) — not capture bugs."

Key files:
bat -n capture/src/norm.rs         # — normalization layer (span/path/file/ty)
bat -n capture/src/project/item.rs # — HIR → NodeKind projection
bat -n capture/src/project/body.rs # — MIR body capture (next target)
bat -n capture/src/assemble.rs     # — partial merge + Use dedup
bat -n model/src/ir/node.rs        # — Body enum (None/Raw/Rust variants)"

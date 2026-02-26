Goal: capture → ModelIR → emit pipeline that produces semantically
equivalent Rust source to the hand-authored ModelIR round-trip.

Current state: structural parity achieved. File layout, module paths,
type names, generics, visibility, and use statements all normalize
correctly. Captured IR round-trips through orchestration and emits
valid file structure.

Remaining gaps (content, not structure):
  - Body::None on all Fn/Method/AssocFn — no MIR body capture yet
  - Const/Static values are empty string — no value capture yet  
  - Trait.methods is empty — HIR trait item association not wired
  - impl Trait / dyn Trait param rendering has minor noise
    (std::marker::Sized in borrow_two, std::Future vs Future)
  - Result1231<T: Sized> — Sized not fully filtered on TyAlias
  - Node emit ordering differs (analyzer-level, not a bug)
  - main.rs not captured (binary entry point, expected gap)

Key files:
  capture/src/norm.rs          — normalization layer (span/path/file/ty)
  capture/src/project/item.rs  — HIR → NodeKind projection
  capture/src/project/body.rs  — MIR body capture (next target)
  capture/src/assemble.rs      — partial merge + Use dedup
  model/src/ir/node.rs         — Body enum (None/Raw/Rust variants)

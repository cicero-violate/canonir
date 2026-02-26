pub mod intern;
pub mod ir;
pub mod node;

pub use intern::{Interner, NameId, PathId};
pub use ir::{CanonIR, CanonNode, TypeKey};
pub use node::{CanonId, CanonNodeKind, CfgOp, PrimTy, TypeKind};

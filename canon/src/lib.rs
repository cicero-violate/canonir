pub mod intern;
pub mod ir;
pub mod node;
pub mod csr_graph;
pub mod edge;
pub mod id;

pub use intern::{Interner, NameId, PathId};
pub use ir::{CanonIR, CanonNode, TypeKey};
pub use node::{CanonId, CanonNodeKind, CfgOp, PrimTy, TypeKind};
pub use edge::EdgeKind;
pub use id::NodeId;

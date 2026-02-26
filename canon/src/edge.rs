use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EdgeKind {
    Renames,
    Resolves,
    TypeOf,
    TypeUnifies,
    ImplTrait,
    DynTrait,
    Calls,
    Contains,
    ImplFor,
    CfgEdge,
    CfgBranch { label: String },
    Outlives,
    ConstDep,
    Expands,
}

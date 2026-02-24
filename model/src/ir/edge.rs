//! Edge kinds for the five constraint graphs.
//!
//! Variables:
//!   src, dst : NodeId   — directed endpoints
//!   EdgeKind            — semantic meaning of the edge
//!
//! Equations (per graph):
//!   G_name  : src --[Renames]--> dst     (rename constraint)
//!   G_type  : src --[TypeOf]-->  dst     (type inference edge)
//!   G_call  : src --[Calls]-->   dst     (call graph edge)
//!   G_module: src --[Contains]--> dst    (module containment)
//!   G_cfg   : src --[CfgEdge]-->  dst    (control flow)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EdgeKind {
    // name-resolution graph
    Renames,
    Resolves,
    // type-inference graph
    TypeOf,
    TypeUnifies,
    // call graph
    Calls,
    // structural / module graph
    Contains, // module contains item
    ImplFor,  // impl -> struct
    // control-flow graph
    CfgEdge,
    CfgBranch { label: String },
    // region / borrow graph
    Outlives,        // lifetime 'a outlives 'b
    // value / const graph
    ConstDep,        // const item depends on another const item
    // macro expansion graph
    Expands,         // macro node expands to item node
}

/// A single edge declaration for use in JSON / capture layer.
/// derive() routes each hint to the correct CsrGraph builder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeHint {
    pub src: u32,
    pub dst: u32,
    pub kind: EdgeKind,
}

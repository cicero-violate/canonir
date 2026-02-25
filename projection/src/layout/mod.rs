use std::path::PathBuf;

use anyhow::Result;
use model::ir::{
    model_ir::ModelIR,
    node::{GenericParam, NodeId, NodeKind},
};

mod passes;
mod skeleton;

pub use passes::{LayoutCtx, LayoutPass};
pub use skeleton::plan_from_ir;

#[derive(Debug, Clone)]
pub struct Plan {
    pub files: Vec<FilePlan>,
}

#[derive(Debug, Clone)]
pub struct FilePlan {
    pub path: PathBuf,
    pub items: Vec<ItemPlan>,
}

#[derive(Debug, Clone)]
pub enum ItemPlan {
    Module(ModuleDeclPlan),
    Impl(ImplPlan),
    CargoToml { name: String, edition: String, has_binary: bool },
    Leaf(NodeKind),
}

#[derive(Debug, Clone)]
pub struct ModuleDeclPlan {
    pub name: String,
    pub inline: bool,
    pub items: Vec<ItemPlan>,
    pub node_id: Option<NodeId>,
}

#[derive(Debug, Clone)]
pub struct ImplPlan {
    pub node_id: Option<NodeId>,
    pub for_struct: String,
    pub for_trait: Option<String>,
    pub generics: Vec<GenericParam>,
    pub attrs: Vec<String>,
    pub where_clauses: Vec<String>,
    pub unsafe_: bool,
    pub methods: Vec<NodeKind>,
}

/// Entry point: build a Plan via structural skeleton + ordered layout passes.
pub fn build_plan(ir: &ModelIR) -> Result<Plan> {
    let mut plan = skeleton::plan_from_ir(ir);
    let ctx = passes::LayoutCtx::new(ir);
    passes::run_layout_passes(&mut plan, &ctx);
    Ok(plan)
}

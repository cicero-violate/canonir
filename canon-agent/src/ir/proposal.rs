use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use std::collections::{HashMap, HashSet};
use thiserror::Error;

use super::{
    ids::{ModuleId, ProposalId, TraitFunctionId, TraitId},
    word::Word,
};

use super::ids::StructId;
use super::word::WordError;

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    #[default]
    Structural,
    FunctionBody,
    SchemaEvolution,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub id: ProposalId,
    #[serde(default)]
    pub kind: ProposalKind,
    pub goal: ProposalGoal,
    pub nodes: Vec<ProposedNode>,
    pub apis: Vec<ProposedApi>,
    pub edges: Vec<ProposedEdge>,
    pub status: ProposalStatus,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ProposalGoal {
    pub id: Word,
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ProposedNode {
    pub id: Option<String>,
    pub name: Word,
    pub module: Option<ModuleId>,
    pub kind: ProposedNodeKind,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ProposedApi {
    pub trait_id: TraitId,
    pub functions: Vec<TraitFunctionId>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ProposedEdge {
    pub from: ModuleId,
    pub to: ModuleId,
    pub rationale: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposedNodeKind {
    Module,
    Struct,
    Trait,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Draft,
    Submitted,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone)]
pub struct ModuleSpec {
    pub id: ModuleId,
    pub name: Word,
}

#[derive(Debug, Clone)]
pub struct StructSpec {
    pub id: StructId,
    pub name: Word,
    pub module: ModuleId,
}

#[derive(Debug, Clone)]
pub struct TraitSpec {
    pub id: TraitId,
    pub name: Word,
    pub module: ModuleId,
}

#[derive(Debug, Clone)]
pub struct ResolvedProposalNodes {
    pub modules: Vec<ModuleSpec>,
    pub structs: Vec<StructSpec>,
    pub traits: Vec<TraitSpec>,
}

#[derive(Debug, Error)]
pub enum ProposalResolutionError {
    #[error("node `{name}` requires a module binding")]
    MissingModule { name: String },
    #[error("unable to determine unique identifier for node `{name}`")]
    MissingIdentifier { name: String },
    #[error("proposal defines duplicate {kind} `{id}`")]
    DuplicateArtifact { kind: &'static str, id: String },
    #[error("identifier `{0}` cannot be converted into a canonical word")]
    InvalidIdentifier(String),
    #[error("word error: {0}")]
    Word(#[from] WordError),
}

pub fn resolve_proposal_nodes(proposal: &Proposal) -> Result<ResolvedProposalNodes, ProposalResolutionError> {
    let mut module_ids = HashSet::new();
    let mut struct_ids = HashSet::new();
    let mut trait_ids = HashSet::new();

    let mut modules = Vec::new();
    let mut structs = Vec::new();
    let mut traits = Vec::new();

    for node in &proposal.nodes {
        match node.kind {
            ProposedNodeKind::Module => {
                let module_id = determine_module_id(node)?;
                if !module_ids.insert(module_id.clone()) {
                    return Err(ProposalResolutionError::DuplicateArtifact { kind: "module", id: module_id });
                }
                modules.push(ModuleSpec { id: module_id, name: node.name.clone() });
            }
            ProposedNodeKind::Struct => {
                let module_id = node.module.clone().ok_or_else(|| ProposalResolutionError::MissingModule { name: node.name.to_string() })?;
                let struct_id = determine_struct_id(node, &module_id)?;
                if !struct_ids.insert(struct_id.clone()) {
                    return Err(ProposalResolutionError::DuplicateArtifact { kind: "struct", id: struct_id });
                }
                structs.push(StructSpec { id: struct_id, name: node.name.clone(), module: module_id });
            }
            ProposedNodeKind::Trait => {
                let module_id = node.module.clone().ok_or_else(|| ProposalResolutionError::MissingModule { name: node.name.to_string() })?;
                let trait_id = determine_trait_id(node, &module_id)?;
                if !trait_ids.insert(trait_id.clone()) {
                    return Err(ProposalResolutionError::DuplicateArtifact { kind: "trait", id: trait_id });
                }
                traits.push(TraitSpec { id: trait_id, name: node.name.clone(), module: module_id });
            }
        }
    }

    Ok(ResolvedProposalNodes { modules, structs, traits })
}

pub fn derive_word_from_identifier(identifier: &str) -> Result<Word, ProposalResolutionError> {
    let trimmed = identifier.rsplit(|c| c == '.' || c == '#').next().unwrap_or(identifier);
    let mut builder = String::new();
    for ch in trimmed.chars() {
        if !ch.is_ascii_alphanumeric() {
            continue;
        }
        if builder.is_empty() {
            if !ch.is_ascii_alphabetic() {
                return Err(ProposalResolutionError::InvalidIdentifier(trimmed.to_owned()));
            }
            builder.push(ch.to_ascii_uppercase());
        } else {
            builder.push(ch);
        }
    }
    if builder.is_empty() {
        return Err(ProposalResolutionError::InvalidIdentifier(trimmed.to_owned()));
    }
    let word = Word::new(builder).map_err(ProposalResolutionError::Word)?;
    Ok(word)
}

pub fn slugify_word(word: &Word) -> String {
    word.as_str().to_ascii_lowercase()
}

pub fn sanitize_identifier(value: &str) -> String {
    value.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect()
}

fn determine_module_id(node: &ProposedNode) -> Result<ModuleId, ProposalResolutionError> {
    if let Some(id) = &node.id {
        return Ok(id.clone());
    }
    if let Some(value) = &node.module {
        return Ok(value.clone());
    }
    Ok(format!("module.{}", slugify_word(&node.name)))
}

fn determine_struct_id(node: &ProposedNode, module_id: &str) -> Result<StructId, ProposalResolutionError> {
    if let Some(id) = &node.id {
        return Ok(id.clone());
    }
    Ok(format!("struct.{}.{}", sanitize_identifier(module_id), slugify_word(&node.name)))
}

fn determine_trait_id(node: &ProposedNode, module_id: &str) -> Result<TraitId, ProposalResolutionError> {
    if let Some(id) = &node.id {
        return Ok(id.clone());
    }
    Ok(format!("trait.{}.{}", sanitize_identifier(module_id), slugify_word(&node.name)))
}

#[derive(Debug, Clone)]
struct DslModule {
    name: String,
    imports: Vec<String>,
}

#[derive(Debug, Clone)]
struct DslGoal {
    name: String,
    steps: Vec<String>,
    signature: String,
}

#[derive(Debug, Clone)]
pub struct DslProposalArtifacts {
    pub proposal: Proposal,
    pub goal_slug: String,
}

#[derive(Debug, Error)]
pub enum DslProposalError {
    #[error("DSL must declare at least one module")]
    MissingModules,
    #[error("DSL must declare a goal line")]
    MissingGoal,
    #[error("invalid module declaration `{0}`")]
    InvalidModule(String),
    #[error("invalid goal declaration `{0}`")]
    InvalidGoal(String),
    #[error("word error: {0}")]
    Word(#[from] WordError),
}

pub fn create_proposal_from_dsl(source: &str) -> Result<DslProposalArtifacts, DslProposalError> {
    let mut modules = Vec::new();
    let mut goal = None;

    for raw_line in source.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("module ") {
            modules.push(parse_module_decl(rest)?);
            continue;
        }
        if let Some(rest) = line.strip_prefix("goal ") {
            goal = Some(parse_goal_decl(rest)?);
            continue;
        }
    }

    if modules.is_empty() {
        return Err(DslProposalError::MissingModules);
    }
    let goal = goal.ok_or(DslProposalError::MissingGoal)?;

    let mut nodes = Vec::new();
    let mut apis = Vec::new();
    let mut edges = Vec::new();
    let mut module_lookup: HashMap<String, String> = HashMap::new();

    for module in &modules {
        let module_word = canonical_word(&module.name)?;
        let module_id = format!("module.{}", sanitize_identifier(&module.name));
        nodes.push(ProposedNode { id: Some(module_id.clone()), name: module_word.clone(), module: None, kind: ProposedNodeKind::Module });

        let struct_word = Word::new(format!("{}State", module_word.as_str()))?;
        let struct_id = format!("struct.{}.state", sanitize_identifier(&module_id));
        nodes.push(ProposedNode { id: Some(struct_id), name: struct_word, module: Some(module_id.clone()), kind: ProposedNodeKind::Struct });

        module_lookup.insert(sanitize_identifier(&module.name), module_id.clone());
    }

    if let Some(first_module) = modules.first() {
        let default_module_id = format!("module.{}", sanitize_identifier(&first_module.name));
        let mut seen_traits = HashSet::new();
        for step in &goal.steps {
            let trait_name = canonical_word(step)?;
            let step_slug = sanitize_identifier(step);
            let module_id = module_lookup.get(&step_slug).cloned().unwrap_or_else(|| default_module_id.clone());
            let trait_id = format!("trait.{}.{}", sanitize_identifier(&module_id), step_slug);
            if seen_traits.insert(trait_id.clone()) {
                nodes.push(ProposedNode { id: Some(trait_id.clone()), name: trait_name, module: Some(module_id.clone()), kind: ProposedNodeKind::Trait });
                let fn_id = format!("trait_fn.{}.{}", sanitize_identifier(&module_id), step_slug);
                apis.push(ProposedApi { trait_id: trait_id.clone(), functions: vec![fn_id] });
            }
        }
    }

    let mut edge_set: HashSet<(String, String)> = HashSet::new();
    for module in &modules {
        let from = match module_lookup.get(&sanitize_identifier(&module.name)) {
            Some(id) => id.clone(),
            None => continue,
        };
        for import in &module.imports {
            if let Some(target) = module_lookup.get(&sanitize_identifier(import)) {
                let key = (from.clone(), target.clone());
                if edge_set.insert(key.clone()) {
                    edges.push(ProposedEdge { from: key.0, to: key.1, rationale: format!("{} imports {}", module.name, import) });
                }
            }
        }
    }

    let goal_id = canonical_word(&goal.name)?;
    let goal_slug = sanitize_identifier(&goal.name);
    let proposal = Proposal {
        id: format!("proposal.dsl.{goal_slug}"),
        kind: ProposalKind::Structural,
        goal: ProposalGoal { id: goal_id, description: format!("Auto-generated from DSL: {}", goal.signature) },
        nodes,
        apis,
        edges,
        status: ProposalStatus::Submitted,
    };

    Ok(DslProposalArtifacts { proposal, goal_slug })
}

fn parse_module_decl(input: &str) -> Result<DslModule, DslProposalError> {
    let mut parts = input.splitn(2, " imports ");
    let name = parts.next().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| DslProposalError::InvalidModule(input.to_string()))?;
    let imports = parts.next().map(|rest| rest.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>()).unwrap_or_default();
    Ok(DslModule { name: name.to_string(), imports })
}

fn parse_goal_decl(input: &str) -> Result<DslGoal, DslProposalError> {
    let (name, signature) =
        input.split_once(':').map(|(lhs, rhs)| (lhs.trim(), rhs.trim())).filter(|(lhs, rhs)| !lhs.is_empty() && !rhs.is_empty()).ok_or_else(|| DslProposalError::InvalidGoal(input.to_string()))?;
    let steps: Vec<String> = signature.split("->").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if steps.is_empty() {
        return Err(DslProposalError::InvalidGoal(input.to_string()));
    }
    Ok(DslGoal { name: name.to_string(), steps, signature: signature.to_string() })
}

fn canonical_word(input: &str) -> Result<Word, WordError> {
    let mut builder = String::new();
    for (idx, ch) in input.chars().filter(|c| c.is_ascii_alphanumeric()).enumerate() {
        if idx == 0 {
            builder.push(ch.to_ascii_uppercase());
        } else {
            builder.push(ch);
        }
    }
    if builder.is_empty() {
        return Err(WordError::Invalid(input.to_string()));
    }
    Word::new(builder)
}

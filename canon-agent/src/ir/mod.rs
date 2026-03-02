mod admission;
mod artifacts;
mod core;
mod delta;
mod errors;
mod functions;
pub mod goals;
mod gpu;
mod graphs;
mod ids;
mod judgment;
mod learning;
mod policy;
mod project;
mod proofs;
pub mod proposal;
mod reward;
mod timeline;
mod types;
pub use types::CodeDelta;
mod word;
pub mod world_model;
pub use admission::{AdmissionPolicy, AppliedDeltaRecord};
pub use artifacts::{
    AssociatedConst, AssociatedType, ConstItem, EnumNode, EnumVariant, EnumVariantFields, Field, ImplBlock, ImplFunctionBinding, Module, ModuleEdge, PubUseItem, StaticItem, Struct, Trait,
    TraitFunction, TypeAlias,
};
pub use core::{CanonicalMeta, Language, PipelineStage, SystemState, VersionContract};
pub use delta::{ChangePayload, DeltaKind, StateChange};
pub use errors::ErrorArtifact;
pub use functions::{DeltaRef, Function, FunctionContract, FunctionMetadata, FunctionSignature, GenericParam, Postcondition, WhereClause};
pub use goals::{GoalDriftMetric, GoalMutation, GoalMutationStatus};
pub use gpu::{GpuFunction, GpuProperties, VectorPort};
pub use graphs::{CallEdge, ExecutionGraph, SystemEdge, SystemEdgeKind, SystemGraph, SystemNode, SystemNodeKind, TickEdge};
pub use ids::*;
pub use judgment::{Decision, JudgmentDecision, Rule};
pub use learning::Learning;
pub use policy::PolicyParameters;
pub use project::{ExternalDependency, Project};
pub use proofs::{Proof, ProofArtifact, ProofScope};
pub use proposal::{
    create_proposal_from_dsl, derive_word_from_identifier, resolve_proposal_nodes, sanitize_identifier, slugify_word, DslProposalArtifacts, DslProposalError, ModuleSpec, ProposalResolutionError,
    ResolvedProposalNodes, StructSpec, TraitSpec,
};
pub use proposal::{Proposal, ProposalGoal, ProposalKind, ProposalStatus, ProposedApi, ProposedEdge, ProposedNode, ProposedNodeKind};
pub use reward::{RewardRecord, UtilityKind};
pub use timeline::{ExecutionEpoch, ExecutionError, ExecutionEvent, ExecutionRecord, LoopPolicy, Plan, Tick};
pub use types::{Receiver, RefKind, ScalarType, StructKind, TypeKind, TypeRef, ValuePort, Visibility};
pub use word::{Word, WordError, WORD_PATTERN};

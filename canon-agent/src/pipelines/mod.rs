//! Pluggable pipeline trait for the canon-agent runner.
//!
//! Each pipeline owns one full agent tick:
//!   - observe the current state
//!   - plan an action (via LLM or deterministic logic)
//!   - act (patch files, mutate IR, run shell commands)
//!   - verify (cargo check, orchestration, surface delta)
//!   - return a scored outcome
//!
//! The runner is pipeline-agnostic — it drives ticks and manages
//! the reward ledger; the pipeline decides what happens inside each tick.

pub mod capability;
pub mod refactor;

use crate::ir::SystemState;
use crate::layout::FileTopology;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Context passed into every pipeline tick
// ---------------------------------------------------------------------------

/// Everything a pipeline tick needs to read and act on the world.
#[derive(Debug, Clone)]
pub struct PipelineContext {
    /// The codebase the agent is allowed to patch (e.g. canon-capture).
    /// Patchable working directories. First entry is primary (used for logs).
    /// All entries are valid patch targets.
    pub cwd: Vec<PathBuf>,
    /// Capture directory containing canon_capture.json — input to orchestration.
    pub capture_dir: PathBuf,
    /// Emit directory where orchestration writes its output.
    pub emit_dir: PathBuf,
    /// Path to the orchestration binary.
    pub orchestration_bin: PathBuf,
    /// Workspace root used for cargo check / cargo build.
    pub workspace: PathBuf,
    /// Current tick number (1-based).
    pub tick: u64,
}

// ---------------------------------------------------------------------------
// Outcome of one pipeline tick
// ---------------------------------------------------------------------------

/// Scalar reward signal from one tick.  Positive = progress, negative = regression.
///
/// The runner uses this to update the NodeRewardLedger regardless of which
/// pipeline produced it.
#[derive(Debug, Clone)]
pub struct PipelineOutcome {
    /// Reward signal for this tick.
    pub reward: f64,
    /// Human-readable summary of what happened.
    pub summary: String,
    /// Whether the tick made a verified forward step.
    pub advanced: bool,
}

// ---------------------------------------------------------------------------
// Pipeline trait
// ---------------------------------------------------------------------------

/// A stateless, async pipeline that runs one agent tick.
///
/// Implementations must be `Send + Sync` so the runner can hold them
/// behind an `Arc<dyn Pipeline>`.
#[async_trait::async_trait]
pub trait Pipeline: Send + Sync {
    /// Human-readable name shown in runner logs.
    fn name(&self) -> &str;

    /// Run one tick.  Receives the shared context and the current IR/layout
    /// (may be ignored by pipelines that operate purely on files).
    async fn run_tick(&self, ctx: &PipelineContext, ir: &mut SystemState, layout: &mut FileTopology) -> anyhow::Result<PipelineOutcome>;
}

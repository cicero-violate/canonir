//! Pluggable pipeline trait for the canon-agent runner.
#[path = "capability_mod.rs"]
pub mod capability;
use crate::ir::SystemState;
use crate::layout::FileTopology;
use std::path::PathBuf;
/// Everything a pipeline tick needs to read and act on the world.
#[derive(Debug, Clone)]
pub struct PipelineContext {
    /// Patchable working directories. First entry is primary (used for logs).
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
/// Scalar reward signal from one tick. Positive = progress, negative = regression.
#[derive(Debug, Clone)]
pub struct PipelineOutcome {
    pub reward: f64,
    pub summary: String,
    pub advanced: bool,
}
/// A stateless, async pipeline that runs one agent tick.
#[async_trait::async_trait]
pub trait Pipeline: Send + Sync {
    fn name(&self) -> &str;
    async fn capability_pipeline_pipeline_run_tick(
        &self,
        ctx: &PipelineContext,
        ir: &mut SystemState,
        layout: &mut FileTopology,
    ) -> anyhow::Result<PipelineOutcome>;
}

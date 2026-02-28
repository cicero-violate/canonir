//! InvariantPipeline — structural invariant discovery loop.
//!
//! Per tick:
//!   1. Observe  — read canon_structural_surface.json from the emit dir.
//!                 If missing, run orchestration first to produce it.
//!   2. Plan     — LLM receives the surface (gap sites + counts) and the
//!                 capture src file for the first unresolved __ret gap.
//!                 It returns a CodeDelta list (apply_patch + bash).
//!   3. Act      — execute the CodeDelta list against the capture dir.
//!   4. Verify   — re-run orchestration, read the new surface.
//!   5. Score    — reward = reduction in unresolved_ret_gap_count.
//!                 Negative reward if count increased or build failed.
//!
//! The pipeline is stateless — all state lives in files on disk.
//! The LLM is called once per tick via the WsBridge.

use super::{Pipeline, PipelineContext, PipelineOutcome};
use crate::ir::{CodeDelta, SystemState};
use crate::layout::FileTopology;
use crate::ws_server::WsBridge;
use crate::emit_shell::emit_shell;
use anyhow::{Context, Result};
use canon_telemetry::StructuralSurface;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

// ---------------------------------------------------------------------------
// LLM request / response shapes
// ---------------------------------------------------------------------------

/// What we send to the LLM each tick.
#[derive(Debug, Serialize)]
struct InvariantPlanRequest {
    /// Structural surface from the last orchestration run.
    surface: StructuralSurface,
    /// Raw source of the first gap file, so the LLM has context.
    gap_file_src: Option<String>,
    /// Instruction to the LLM.
    instruction: String,
}

/// What we expect the LLM to return.
#[derive(Debug, Deserialize)]
struct InvariantPlanResponse {
    /// List of deltas to apply to the capture src directory.
    deltas: Vec<CodeDelta>,
    /// LLM's rationale for the patch.
    rationale: String,
}

// ---------------------------------------------------------------------------
// Pipeline implementation
// ---------------------------------------------------------------------------

pub struct InvariantPipeline {
    pub bridge: WsBridge,
}

#[async_trait::async_trait]
impl Pipeline for InvariantPipeline {
    fn name(&self) -> &str {
        "invariant"
    }

    async fn run_tick(
        &self,
        ctx: &PipelineContext,
        _ir: &mut SystemState,
        _layout: &mut FileTopology,
    ) -> Result<PipelineOutcome> {
        // ----------------------------------------------------------------
        // 1. Observe — get current surface, running orchestration if needed
        // ----------------------------------------------------------------
        let surface_before = observe(ctx)
            .context("observe: failed to get structural surface")?;

        eprintln!(
            "[invariant] tick {} — surface: {} suppressed, {} __ret gaps, build={}",
            ctx.tick,
            surface_before.suppressed_count,
            surface_before.unresolved_ret_gap_count,
            if canon_telemetry::build(&ctx.emit_dir, true)
                .map(|r| r.success)
                .unwrap_or(false) { "OK" } else { "FAIL" }
        );

        if surface_before.unresolved_ret_gap_count == 0 {
            return Ok(PipelineOutcome {
                reward: 1.0,
                summary: "no unresolved __ret gaps — done".into(),
                advanced: false,
            });
        }

        // ----------------------------------------------------------------
        // 2. Plan — ask the LLM for a patch
        // ----------------------------------------------------------------
        let gap_file_src = surface_before
            .ret_gap_sites
            .first()
            .and_then(|site| {
                let p = ctx.capture_dir.join("src").join(&site.file);
                std::fs::read_to_string(&p).ok()
            });

        let request = InvariantPlanRequest {
            surface: surface_before.clone(),
            gap_file_src,
            instruction: format!(
                "You are fixing unresolved __ret gaps in canon-emitted Rust source. \
                 The capture src is at {:?}. \
                 Return JSON with fields: \
                 'deltas' (array of {{\"ApplyPatch\":{{\"patch\":\"...\"}}}} or \
                 {{\"Bash\":{{\"command\":\"...\"}}}} objects) and 'rationale' (string). \
                 Target the first gap site: {}. \
                 Only emit deltas that modify files under {:?}/src/.",
                ctx.capture_dir,
                surface_before.ret_gap_sites.first()
                    .map(|s| format!("{}:{} {}", s.file, s.line, s.enclosing_fn))
                    .unwrap_or_default(),
                ctx.capture_dir,
            ),
        };

        let plan = plan_via_llm(&self.bridge, &request, ctx.tick).await
            .context("plan: LLM call failed")?;

        eprintln!("[invariant] tick {} — rationale: {}", ctx.tick, plan.rationale);
        eprintln!("[invariant] tick {} — {} delta(s) to apply", ctx.tick, plan.deltas.len());

        // ----------------------------------------------------------------
        // 3. Act — execute deltas against the capture dir
        // ----------------------------------------------------------------
        act(&plan.deltas, &ctx.capture_dir)
            .context("act: delta execution failed")?;

        // ----------------------------------------------------------------
        // 4. Verify — re-run orchestration, read new surface
        // ----------------------------------------------------------------
        run_orchestration(ctx).context("verify: orchestration failed")?;
        let surface_after = read_surface(&ctx.emit_dir)
            .context("verify: could not read surface after orchestration")?;

        let build_ok = canon_telemetry::build(&ctx.emit_dir, true)
            .map(|r| r.success)
            .unwrap_or(false);

        // ----------------------------------------------------------------
        // 5. Score
        // ----------------------------------------------------------------
        let gaps_before = surface_before.unresolved_ret_gap_count as i64;
        let gaps_after  = surface_after.unresolved_ret_gap_count as i64;
        let delta       = gaps_before - gaps_after; // positive = progress

        let reward = score(delta, build_ok);
        let advanced = delta > 0 && build_ok;

        let summary = format!(
            "__ret gaps: {} → {}  (Δ={})  build={}  reward={:.3}",
            gaps_before, gaps_after, delta,
            if build_ok { "OK" } else { "FAIL" },
            reward,
        );
        eprintln!("[invariant] tick {} — {}", ctx.tick, summary);

        Ok(PipelineOutcome { reward, summary, advanced })
    }
}

// ---------------------------------------------------------------------------
// Observe
// ---------------------------------------------------------------------------

/// Read the structural surface from disk, running orchestration first if the
/// surface snapshot is absent.
fn observe(ctx: &PipelineContext) -> Result<StructuralSurface> {
    let surface_path = ctx.emit_dir.join("canon_structural_surface.json");
    if !surface_path.exists() {
        run_orchestration(ctx).context("observe: initial orchestration run failed")?;
    }
    read_surface(&ctx.emit_dir)
}

fn read_surface(emit_dir: &Path) -> Result<StructuralSurface> {
    let path = emit_dir.join("canon_structural_surface.json");
    let json = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {:?}", path))?;
    serde_json::from_str(&json)
        .with_context(|| format!("cannot parse StructuralSurface from {:?}", path))
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

fn run_orchestration(ctx: &PipelineContext) -> Result<()> {
    let capture_json = ctx.capture_dir.join("canon_capture.json");
    let status = Command::new(&ctx.orchestration_bin)
        .arg(&capture_json)
        .arg(&ctx.emit_dir)
        .status()
        .with_context(|| format!("failed to spawn {:?}", ctx.orchestration_bin))?;
    anyhow::ensure!(status.success(), "orchestration exited with {}", status);
    Ok(())
}

// ---------------------------------------------------------------------------
// Plan via LLM
// ---------------------------------------------------------------------------

async fn plan_via_llm(
    bridge: &WsBridge,
    request: &InvariantPlanRequest,
    tick: u64,
) -> Result<InvariantPlanResponse> {
    use crate::call::AgentCallInput;
    let payload = serde_json::to_value(request)?;
    let input = AgentCallInput {
        call_id: format!("invariant-tick-{tick}"),
        node_id: "invariant-planner".into(),
        ir_slice: serde_json::Value::Null,
        predecessor_outputs: vec![],
        stage: crate::ir::PipelineStage::Act,
    };

    // We inject the full request as the system prompt context via bridge.
    // call_llm sends input to the extension; extension returns AgentCallOutput.
    let output = crate::llm_provider::call_llm(bridge, &input, None).await
        .map_err(|e| anyhow::anyhow!("llm error: {e}"))?;

    // The LLM must return JSON with 'deltas' and 'rationale' in its payload.
    let response: InvariantPlanResponse = serde_json::from_value(output.payload)
        .context("LLM payload did not match InvariantPlanResponse schema")?;

    Ok(response)
}

// ---------------------------------------------------------------------------
// Act
// ---------------------------------------------------------------------------

/// Execute a list of CodeDeltas against the capture directory.
/// ApplyPatch deltas are run via the `apply_patch` tool.
/// Bash deltas are run via sh with the capture dir as cwd.
fn act(deltas: &[CodeDelta], capture_dir: &Path) -> Result<()> {
    for delta in deltas {
        match delta {
            CodeDelta::Bash { command } => {
                eprintln!("[invariant] bash: {}", command.lines().next().unwrap_or(""));
                let status = Command::new("bash")
                    .arg("-c")
                    .arg(command)
                    .current_dir(capture_dir)
                    .status()
                    .context("bash command failed to spawn")?;
                anyhow::ensure!(status.success(), "bash command exited with {}", status);
            }
            CodeDelta::ApplyPatch { patch } => {
                eprintln!("[invariant] apply_patch ({} bytes)", patch.len());
                // Write patch to a temp file and invoke apply_patch.
                let tmp = std::env::temp_dir().join(format!("canon_patch_{}.patch", std::process::id()));
                std::fs::write(&tmp, patch)?;
                let status = Command::new("apply_patch")
                    .arg(&tmp)
                    .current_dir(capture_dir)
                    .status()
                    .context("apply_patch failed to spawn")?;
                let _ = std::fs::remove_file(&tmp);
                anyhow::ensure!(status.success(), "apply_patch exited with {}", status);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Score
// ---------------------------------------------------------------------------

/// Reward function.
///
/// ```
/// reward = delta * 0.5 + build_bonus
/// ```
///
/// where delta = gaps_before - gaps_after (positive = progress)
/// and build_bonus = +0.2 if build OK, -0.3 if build failed.
fn score(gap_delta: i64, build_ok: bool) -> f64 {
    let build_bonus = if build_ok { 0.2 } else { -0.3 };
    (gap_delta as f64) * 0.5 + build_bonus
}

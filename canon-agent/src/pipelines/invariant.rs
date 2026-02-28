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
use crate::llm_provider::call_llm_raw;
use crate::ws_server::WsBridge;
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
    pub chatgpt_url: String,
    bootstrapped: std::sync::Mutex<bool>,
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
            if cargo_check(&ctx.cwd) { "OK" } else { "FAIL" }
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
                let p = ctx.emit_dir.join("src").join(&site.file);
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
                ctx.cwd,
                surface_before.ret_gap_sites.first()
                    .map(|s| format!("{}:{} {}", s.file, s.line, s.enclosing_fn))
                    .unwrap_or_default(),
                ctx.cwd,
            ),
        };

        let plan = plan_via_llm(&self.bridge, &self.chatgpt_url, &request, ctx.tick, &ctx.cwd).await
            .context("plan: LLM call failed")?;

        eprintln!("[invariant] tick {} — rationale: {}", ctx.tick, plan.rationale);
        eprintln!("[invariant] tick {} — {} delta(s) to apply", ctx.tick, plan.deltas.len());

        // ----------------------------------------------------------------
        // 3. Act — execute deltas against the capture dir
        // ----------------------------------------------------------------
        if let Err(e) = act(&plan.deltas, &ctx.cwd) {
            eprintln!("[invariant] tick {} — act failed: {e}, retrying with error context", ctx.tick);
            match plan_via_llm_with_error(&self.bridge, &self.chatgpt_url, &request, ctx.tick, &ctx.cwd, &e.to_string()).await {
                Ok(retry_plan) => {
                    eprintln!("[invariant] tick {} — retry rationale: {}", ctx.tick, retry_plan.rationale);
                    if let Err(e2) = act(&retry_plan.deltas, &ctx.cwd) {
                        eprintln!("[invariant] tick {} — retry also failed: {e2}, skipping tick", ctx.tick);
                    }
                }
                Err(e2) => {
                    eprintln!("[invariant] tick {} — retry LLM call failed: {e2}, skipping tick", ctx.tick);
                }
            }
        }

        // ----------------------------------------------------------------
        // 4. Verify — re-run orchestration, read new surface
        // ----------------------------------------------------------------
        run_orchestration(ctx).context("verify: orchestration failed")?;
        let surface_after = read_surface(&ctx.emit_dir)
            .context("verify: could not read surface after orchestration")?;

        let build_ok = cargo_check(&ctx.cwd);

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
    url: &str,
    request: &InvariantPlanRequest,
    tick: u64,
    capture_dir: &Path,
) -> Result<InvariantPlanResponse> {
    let surface_json = serde_json::to_string_pretty(&request.surface)?;
    let gap_src = request.gap_file_src.as_deref().unwrap_or("(source unavailable)");
    let first_gap = request
        .surface
        .ret_gap_sites
        .first()
        .map(|s| format!("{}:{} — {}", s.file, s.line, s.enclosing_fn))
        .unwrap_or_else(|| "(none)".into());

    let agent_goal = std::fs::read_to_string(
        "/workspace/ai_sandbox/canon/canon-agent-tools/AGENT_GOAL.md",
    )
    .unwrap_or_else(|e| format!("(could not load AGENT_GOAL.md: {e})"));

    let patch_format = std::fs::read_to_string(
        "/workspace/ai_sandbox/canon/canon-agent-tools/gpt_5_2_prompt_apply_patch.md",
    )
    .unwrap_or_else(|e| format!("(could not load patch format doc: {e})"));

    let mir_sources = load_dir_sources(&capture_dir.join("src/capture/mir"));
    let mir_sources = load_specific_sources(capture_dir, &[
        "src/capture/mir/terminator.rs",
        "src/capture/mir/passes.rs",
        "src/capture/mir/lower.rs",
        "src/capture/mir/guard.rs",
        "src/capture/mir/util.rs",
    ]);

    let prompt = format!(
        agent_goal = agent_goal,
        tick = tick,
        surface_json = surface_json,
        first_gap = first_gap,
        gap_src = gap_src,
        cwd = capture_dir.display(),
        mir_sources = mir_sources,
        patch_format = patch_format,
    );

    let payload = call_llm_raw(bridge, prompt, url)
        .await
        .map_err(|e| anyhow::anyhow!("llm error: {e}"))?;

    let response: InvariantPlanResponse = serde_json::from_value(payload)
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
               // Unescape literal \n sequences the LLM emits inside JSON strings,
               // then write to a temp file and invoke apply_patch.
               let expanded = patch.replace("\\n", "\n").replace("\\t", "\t");
               use std::io::Write as _;
               eprintln!("[invariant] apply_patch raw repr: {:?}", &expanded[..expanded.len().min(200)]);
               let mut child = Command::new("apply_patch")
                   .stdin(std::process::Stdio::piped())
                   .current_dir(capture_dir)
                   .spawn()
                   .context("apply_patch failed to spawn")?;
               if let Some(mut stdin) = child.stdin.take() {
                   stdin.write_all(expanded.as_bytes())
                       .context("apply_patch: failed to write patch to stdin")?;
               }
               let out = child.wait_with_output()
                   .context("apply_patch: wait failed")?;
               anyhow::ensure!(out.status.success(), "apply_patch exited with {}", out.status);
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

fn build_delta_prompt(
    surface: &StructuralSurface,
    tick: u64,
    build_ok: bool,
) -> String {
    let first_gap = surface
        .ret_gap_sites
        .first()
        .map(|s| format!("{}:{} — {}", s.file, s.line, s.enclosing_fn))
        .unwrap_or_else(|| "(none)".into());

    format!(
        tick = tick,
        gaps = surface.unresolved_ret_gap_count,
        build = if build_ok { "OK" } else { "FAIL" },
        first_gap = first_gap,
    )
}

/// Load all `.rs` files from `dir` into a single formatted string for the prompt.
fn load_dir_sources(dir: &Path) -> String {
    let mut out = String::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            return format!("(could not read {:?}: {e})", dir);
        }
    };
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("rs"))
        .collect();
    paths.sort();
    for path in paths {
        let rel = path
            .strip_prefix(dir.parent().and_then(|p| p.parent()).unwrap_or(dir))
            .map(|r| r.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| format!("(read error: {e})"));
        out.push_str(&format!("### {rel}\n```rust\n{src}\n```\n\n"));
    }
    out
}

/// Load specific source files relative to `base` into a prompt string.
fn load_specific_sources(base: &Path, rel_paths: &[&str]) -> String {
    let mut out = String::new();
    for rel in rel_paths {
        let path = base.join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| format!("(read error: {e})"));
        out.push_str(&format!("### {rel}\n```rust\n{src}\n```\n\n"));
    }
    out
}

async fn plan_via_llm_with_error(
    bridge: &WsBridge,
    url: &str,
    request: &InvariantPlanRequest,
    tick: u64,
    capture_dir: &Path,
    error: &str,
) -> Result<InvariantPlanResponse> {
    let agent_goal = std::fs::read_to_string(
        "/workspace/ai_sandbox/canon/canon-agent-tools/AGENT_GOAL.md",
    )
    .unwrap_or_else(|e| format!("(could not load AGENT_GOAL.md: {e})"));

    let patch_format = std::fs::read_to_string(
        "/workspace/ai_sandbox/canon/canon-agent-tools/gpt_5_2_prompt_apply_patch.md",
    )
    .unwrap_or_else(|e| format!("(could not load patch format: {e})"));

    let mir_sources = load_specific_sources(capture_dir, &[
        "src/capture/mir/terminator.rs",
        "src/capture/mir/passes.rs",
        "src/capture/mir/lower.rs",
        "src/capture/mir/guard.rs",
        "src/capture/mir/util.rs",
    ]);

    let surface_json = serde_json::to_string_pretty(&request.surface)?;
    let gap_src = request.gap_file_src.as_deref().unwrap_or("(source unavailable)");
    let first_gap = request
        .surface
        .ret_gap_sites
        .first()
        .map(|s| format!("{}:{} — {}", s.file, s.line, s.enclosing_fn))
        .unwrap_or_else(|| "(none)".into());

    let prompt = format!(
        agent_goal = agent_goal,
        tick = tick,
        error = error,
        surface_json = surface_json,
        first_gap = first_gap,
        gap_src = gap_src,
        cwd = capture_dir.display(),
        mir_sources = mir_sources,
        patch_format = patch_format,
    );

    let payload = call_llm_raw(bridge, prompt, url)
        .await
        .map_err(|e| anyhow::anyhow!("llm error: {e}"))?;

    let response: InvariantPlanResponse = serde_json::from_value(payload)
        .context("LLM retry payload did not match InvariantPlanResponse schema")?;

    Ok(response)
}
/// Run `cargo check` in `dir`, return true if it succeeds.
fn cargo_check(dir: &Path) -> bool {
    Command::new("cargo")
        .args(["check"])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

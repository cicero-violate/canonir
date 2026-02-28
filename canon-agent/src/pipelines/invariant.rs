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

use std::path::PathBuf;
use super::{Pipeline, PipelineContext, PipelineOutcome};
use crate::ir::{CodeDelta, SystemState};
use crate::layout::FileTopology;
use crate::llm_provider::call_llm_raw;
use crate::ws_server::WsBridge;
use anyhow::{Context, Result};
use canon_telemetry::{StructuralSurface, scan_emit_dir, build};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

// ---------------------------------------------------------------------------
// Prompt configuration (loaded from prompt_config.toml)
// ---------------------------------------------------------------------------

const PROMPT_CONFIG_TOML: &str =
    "/workspace/ai_sandbox/canon/canon-agent-tools/prompt_config.toml";
const AGENT_TOOLS_DIR: &str =
    "/workspace/ai_sandbox/canon/canon-agent-tools";

#[derive(Debug, serde::Deserialize)]
struct RawPromptConfig {
    invariant: InvariantPromptConfig,
}

#[derive(Debug, serde::Deserialize)]
struct InvariantPromptConfig {
    bootstrap_template_path: String,
    delta_template_path: String,
    retry_addendum: String,
    agent_goal_path: String,
    patch_format_path: String,
    mir_sources: Vec<String>,
    instruction: String,
}

struct PromptConfig {
    bootstrap_template: String,
    delta_template: String,
    /// Raw retry addendum template (contains {{RETRY_ERROR}})
    retry_addendum: String,
    agent_goal: String,
    patch_format: String,
    mir_source_relpaths: Vec<String>,
    instruction: String,
}

impl PromptConfig {
    fn load() -> Result<Self> {
        let raw_toml = std::fs::read_to_string(PROMPT_CONFIG_TOML)
            .with_context(|| format!("cannot read {}", PROMPT_CONFIG_TOML))?;
        let raw: RawPromptConfig = toml::from_str(&raw_toml)
            .context("cannot parse prompt_config.toml")?;
        let cfg = raw.invariant;

        let tools = std::path::Path::new(AGENT_TOOLS_DIR);

        let bootstrap_template = std::fs::read_to_string(tools.join(&cfg.bootstrap_template_path))
            .with_context(|| format!("cannot read bootstrap template: {}", cfg.bootstrap_template_path))?;
        let delta_template = std::fs::read_to_string(tools.join(&cfg.delta_template_path))
            .with_context(|| format!("cannot read delta template: {}", cfg.delta_template_path))?;
        let agent_goal = std::fs::read_to_string(tools.join(&cfg.agent_goal_path))
            .unwrap_or_else(|e| format!("(could not load agent_goal: {e})"));
        let patch_format = std::fs::read_to_string(tools.join(&cfg.patch_format_path))
            .unwrap_or_else(|e| format!("(could not load patch_format: {e})"));

        Ok(Self {
            bootstrap_template,
            delta_template,
            retry_addendum: cfg.retry_addendum,
            agent_goal,
            patch_format,
            mir_source_relpaths: cfg.mir_sources,
            instruction: cfg.instruction,
        })
    }

    /// Render the primary prompt for this tick (bootstrap or delta).
    fn render_primary(
        &self,
        tick: u64,
        surface_json: &str,
        first_gap: &str,
        gap_src: &str,
        mir_sources: &str,
        capture_dir: &Path,
        delta_feedback: &str,
        last_patch_diff_summary: &str,
        gap_count: u64,
    ) -> String {
        if tick == 0 {
            self.bootstrap_template
                .replace("{{TICK}}", &tick.to_string())
                .replace("{{SURFACE}}", surface_json)
                .replace("{{TARGET_GAP}}", first_gap)
                .replace("{{EMITTED_SRC}}", gap_src)
                .replace("{{MIR_SRC}}", mir_sources)
                .replace("{{CWD}}", &capture_dir.display().to_string())
                .replace("{{STRUCTURAL_DELTA_FEEDBACK}}", delta_feedback)
                .replace("{{LAST_PATCH_DIFF_SUMMARY}}", last_patch_diff_summary)
                .replace("{{AGENT_GOAL}}", &self.agent_goal)
                .replace("{{PATCH_FORMAT}}", &self.patch_format)
        } else {
            // Tick > 0: compact status + current patchable capture sources.
            // Tab is persistent — LLM already has sources from tick 0.
            format!(
                "TICK {tick} | gaps remaining: {gaps} | next: {gap}\n\
                 delta feedback: {feedback}\n\
                 last patch diff: {diff}",
                tick = tick,
                gaps = gap_count,
                gap = first_gap,
                feedback = delta_feedback,
                diff = last_patch_diff_summary,
            )
        }
    }

    fn render_bootstrap(
        &self,
        tick: u64,
        surface_json: &str,
        first_gap: &str,
        gap_src: &str,
        mir_sources: &str,
        capture_dir: &Path,
        delta_feedback: &str,
        last_patch_diff_summary: &str,
        gap_count: u64,
    ) -> String {
        self.bootstrap_template
            .replace("{{TICK}}", &tick.to_string())
            .replace("{{SURFACE}}", surface_json)
            .replace("{{TARGET_GAP}}", first_gap)
            .replace("{{EMITTED_SRC}}", gap_src)
            .replace("{{MIR_SRC}}", mir_sources)
            .replace("{{CWD}}", &capture_dir.display().to_string())
            .replace("{{STRUCTURAL_DELTA_FEEDBACK}}", delta_feedback)
            .replace("{{LAST_PATCH_DIFF_SUMMARY}}", last_patch_diff_summary)
            .replace("{{AGENT_GOAL}}", &self.agent_goal)
            .replace("{{PATCH_FORMAT}}", &self.patch_format)
    }

    fn render_delta(
        &self,
        tick: u64,
        first_gap: &str,
        delta_feedback: &str,
        last_patch_diff_summary: &str,
        gap_count: u64,
        gap_src: &str,
	current_patchable_sources: &str,
    ) -> String {
        self.delta_template
            .replace("{{TICK}}", &tick.to_string())
            .replace("{{GAP_COUNT}}", &gap_count.to_string())
            .replace("{{TARGET_GAP}}", first_gap)
            .replace("{{STRUCTURAL_DELTA_FEEDBACK}}", delta_feedback)
           .replace("{{LAST_PATCH_DIFF_SUMMARY}}", last_patch_diff_summary)
            .replace("{{EMITTED_SRC}}", gap_src)
            .replace("{{MIR_SRC}}", current_patchable_sources)
    }
    /// Render a retry addendum — appended to the original prompt.
    /// Does NOT re-send surface, MIR, or any bootstrap context.
    fn render_retry_addendum(&self, error: &str) -> String {
        self.retry_addendum.replace("{{RETRY_ERROR}}", error)
    }

    /// Load MIR sources relative to capture_dir.
    fn load_mir_sources(&self, capture_dir: &Path) -> String {
        load_specific_sources(capture_dir, &self.mir_source_relpaths.iter().map(|s| s.as_str()).collect::<Vec<_>>())
    }

    fn load_mir_sources_all(&self, capture_dirs: &[std::path::PathBuf]) -> String {
        capture_dirs.iter()
            .map(|dir| self.load_mir_sources(dir))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

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
    config: PromptConfig,
    /// Whether the bootstrap has been sent on this tab session.
    bootstrap_sent: tokio::sync::Mutex<bool>,
    /// Persistent ChatGPT tab — opened once, reused every tick so conversation
    /// history is preserved across ticks.
    tab_id: tokio::sync::Mutex<Option<u32>>,
}

impl InvariantPipeline {
    pub fn new(bridge: WsBridge, chatgpt_url: String) -> Self {
        let config = PromptConfig::load().expect("failed to load prompt_config.toml");
        Self {
            bridge,
            chatgpt_url,
            bootstrapped: std::sync::Mutex::new(false),
            config,
            bootstrap_sent: tokio::sync::Mutex::new(false),
            tab_id: tokio::sync::Mutex::new(None),
        }
    }
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
        let surface_before = canon_telemetry::scan_emit_dir(&ctx.emit_dir)?
            .ok_or_else(|| anyhow::anyhow!("emit/src not found"))?;

        // Persist structural surface snapshot
        let log_dir = ctx.cwd[0].join("invariant_logs").join(format!("tick_{}", ctx.tick));
        std::fs::create_dir_all(&log_dir).ok();
        if let Ok(pretty) = serde_json::to_string_pretty(&surface_before) {
            std::fs::write(log_dir.join("surface_before.json"), pretty).ok();
        }

        let build_before = canon_telemetry::build(&ctx.emit_dir, true)?;

        eprintln!(
            "[invariant] tick {} — surface: {} suppressed, {} __ret gaps, build={}",
            ctx.tick,
            surface_before.suppressed_count,
            surface_before.unresolved_ret_gap_count,
            if build_before.success { "OK" } else { "FAIL" }
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
            instruction: self.config.instruction
                .replace("{{CWD}}", &format!("{:?}", ctx.cwd))
                .replace("{{TARGET_GAP}}", &surface_before.ret_gap_sites.first()
                    .map(|s| format!("{}:{} {}", s.file, s.line, s.enclosing_fn))
                    .unwrap_or_default()),
        };

        // ----------------------------------------------------------------
        // 3. Multi-step Plan/Inspect/Patch loop (bounded)
        // ----------------------------------------------------------------
        const MAX_STEPS: usize = 3;
        let mut last_error: Option<String> = None;
        let mut last_prompt: Option<String> = None;

        for step in 0..MAX_STEPS {
            let plan_result = if let (Some(err), Some(base_prompt)) = (&last_error, &last_prompt) {
                // Retry: send original prompt + delta addendum only
                plan_via_llm_retry(
                    &self.bridge,
                    &self.chatgpt_url,
                    base_prompt,
                    err,
                    &self.config,
                    ctx.tick,
                    &log_dir,
                    &self.tab_id,
                )
                .await
            } else {
                plan_via_llm(
                    &self.bridge,
                    &self.chatgpt_url,
                    &request,
                    ctx.tick,
                    &ctx.cwd[0],
                    &self.config,
                    &mut last_prompt,
                    &log_dir,
                    &self.tab_id,
                    &self.bootstrap_sent,
                    &ctx.cwd,
                )
                .await
            };

        let plan = match plan_result {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[invariant] guardrail rejection: {}", e);
                last_error = Some(e.to_string());
                continue; // retry inside bounded loop
            }
        };

            eprintln!(
                "[invariant] tick {} step {} — rationale: {}",
                ctx.tick, step, plan.rationale
            );

            // If all deltas are read-only Bash, treat as inspection and continue
            let all_readonly = plan.deltas.iter().all(|d| {
                matches!(d, CodeDelta::BashReadOnly { .. })
            });

            match act(&plan.deltas, &ctx.cwd) {
                Ok(_) => {
                    if all_readonly {
                        // Continue loop to allow refinement after inspection
                        continue;
                    } else {
                        // Patch applied — exit loop
                        break;
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[invariant] tick {} step {} — act failed: {}",
                        ctx.tick, step, e
                    );
                    last_error = Some(e.to_string());
                    continue;
                }
            }
        }

        // ----------------------------------------------------------------
        // 4. Verify — re-run orchestration, read new surface
        // ----------------------------------------------------------------
        run_orchestration(ctx)?;

        let surface_after = canon_telemetry::scan_emit_dir(&ctx.emit_dir)?
            .ok_or_else(|| anyhow::anyhow!("emit/src not found after orchestration"))?;

        let build_after = canon_telemetry::build(&ctx.emit_dir, true)?;
        let build_ok = build_after.success;

        // ----------------------------------------------------------------
        // 5. Score
        // ----------------------------------------------------------------
        let gaps_before = surface_before.unresolved_ret_gap_count as i64;
        let gaps_after  = surface_after.unresolved_ret_gap_count as i64;
        let delta       = gaps_before - gaps_after; // positive = progress

        // ------------------------------------------------------------
        // Structural Delta Feedback (localized gap analysis)
        // ------------------------------------------------------------
        use std::collections::HashMap;

        let mut per_file_before: HashMap<String, i64> = HashMap::new();
        let mut per_file_after:  HashMap<String, i64> = HashMap::new();

        let mut per_fn_before: HashMap<String, i64> = HashMap::new();
        let mut per_fn_after:  HashMap<String, i64> = HashMap::new();

        for site in &surface_before.ret_gap_sites {
            *per_file_before.entry(site.file.clone()).or_default() += 1;
            *per_fn_before
                .entry(format!("{}::{}", site.file, site.enclosing_fn))
                .or_default() += 1;
        }

        for site in &surface_after.ret_gap_sites {
            *per_file_after.entry(site.file.clone()).or_default() += 1;
            *per_fn_after
                .entry(format!("{}::{}", site.file, site.enclosing_fn))
                .or_default() += 1;
        }

        let mut per_file_ret_delta = Vec::new();
        for (file, before) in &per_file_before {
            let after = per_file_after.get(file).copied().unwrap_or(0);
            per_file_ret_delta.push((
                file.clone(),
                before - after,
            ));
        }

        let mut per_fn_ret_delta = Vec::new();
        for (k, before) in &per_fn_before {
            let after = per_fn_after.get(k).copied().unwrap_or(0);
            per_fn_ret_delta.push((
                k.clone(),
                before - after,
            ));
        }

        let suppressed_ret_delta =
            (surface_before.suppressed_count as i64)
            - (surface_after.suppressed_count as i64);

        // Persist structural delta feedback for next tick
        let delta_feedback = serde_json::json!({
            "per_file_ret_delta": per_file_ret_delta,
            "per_fn_ret_delta": per_fn_ret_delta,
            "suppressed_ret_delta": suppressed_ret_delta
        });

        std::fs::write(
            log_dir.join("structural_delta_feedback.json"),
            serde_json::to_string_pretty(&delta_feedback).unwrap_or_default(),
        ).ok();

        // Write last_patch_diff_summary for next tick
        let mut diff_lines = Vec::new();
        for (file, d) in &per_file_ret_delta {
            if *d != 0 {
                diff_lines.push(format!("{file}: {d:+}"));
            }
        }
        for (k, d) in &per_fn_ret_delta {
            if *d != 0 {
                diff_lines.push(format!("  fn {k}: {d:+}"));
            }
        }
        if suppressed_ret_delta != 0 {
            diff_lines.push(format!("suppressed: {:+}", suppressed_ret_delta));
        }
        let diff_summary = if diff_lines.is_empty() {
            format!("no change (gaps_before={gaps_before} gaps_after={gaps_after})")
        } else {
            diff_lines.join("\n")
        };
        std::fs::write(log_dir.join("last_patch_diff_summary.txt"), &diff_summary).ok();

        // Penalize stagnation even if build succeeds.
        let reward = if delta == 0 && build_ok {
            -0.1
        } else {
            score(delta, build_ok)
        };
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

/// Get the persistent tab for this pipeline session, opening it if needed.
async fn get_or_open_tab(
    bridge: &WsBridge,
    url: &str,
    tab_id_slot: &tokio::sync::Mutex<Option<u32>>,
) -> Result<u32> {
    let mut slot = tab_id_slot.lock().await;
    if let Some(id) = *slot {
        return Ok(id);
    }
    bridge.wait_for_connection().await;
    let id = bridge
        .open_fresh_tab_with_url(url.to_string())
        .await
        .map_err(|e| anyhow::anyhow!("failed to open tab: {e}"))?;
    *slot = Some(id);
    eprintln!("[invariant] opened persistent tab_id={}", id);
    Ok(id)
}

async fn plan_via_llm(
    bridge: &WsBridge,
    url: &str,
    request: &InvariantPlanRequest,
    tick: u64,
    capture_dir: &Path,  // primary dir (for logs/prev feedback)
    config: &PromptConfig,
    last_prompt_out: &mut Option<String>,
    log_dir: &Path,
    tab_id_slot: &tokio::sync::Mutex<Option<u32>>,
    bootstrap_sent: &tokio::sync::Mutex<bool>,
    all_capture_dirs: &[std::path::PathBuf],
) -> Result<InvariantPlanResponse> {
    let surface_json = serde_json::to_string_pretty(&request.surface)?;
    let gap_src = request.gap_file_src.as_deref().unwrap_or("(source unavailable)");
    let first_gap = request
        .surface
        .ret_gap_sites
        .first()
        .map(|s| format!("{}:{} — {}", s.file, s.line, s.enclosing_fn))
        .unwrap_or_else(|| "(none)".into());

    let mir_sources = config.load_mir_sources_all(all_capture_dirs);

    let prev_tick = tick.saturating_sub(1);

    // prev_log_dir lives under cwd/invariant_logs — same root as log_dir
    let prev_log_dir = log_dir
        .parent()
        .unwrap_or(log_dir)
        .join(format!("tick_{}", prev_tick));

    let delta_feedback = std::fs::read_to_string(
        prev_log_dir.join("structural_delta_feedback.json"),
    )
    .unwrap_or_else(|_| "{}".into());

    let last_patch_diff_summary = std::fs::read_to_string(
        prev_log_dir.join("last_patch_diff_summary.txt"),
    )
    .unwrap_or_else(|_| "(none)".into());

    let prompt = {
        let mut sent = bootstrap_sent.lock().await;
        if !*sent {
            *sent = true;
            config.render_bootstrap(
                tick,
                &surface_json,
                &first_gap,
                gap_src,
                &mir_sources,
                capture_dir,
                &delta_feedback,
                &last_patch_diff_summary,
                request.surface.unresolved_ret_gap_count as u64,
            )
        } else {
            config.render_delta(
                tick,
                &first_gap,
                &delta_feedback,
                &last_patch_diff_summary,
                request.surface.unresolved_ret_gap_count as u64,
                gap_src,
                &mir_sources,
            )
        }
    };

    // Store for retry path — retry appends to this, not re-sends it
    *last_prompt_out = Some(prompt.clone());

    let tab_id = get_or_open_tab(bridge, url, tab_id_slot).await?;
    let raw = bridge
        .send_turn(tab_id, prompt.clone())
        .await
        .map_err(|e| anyhow::anyhow!("llm send_turn error: {e}"))?;
    let payload = crate::llm_provider::JsonExtractor::extract(&raw)
        .map_err(|e| anyhow::anyhow!("json extract error: {e}"))?;

    // ------------------------------
    // Persist prompt + raw response
    // ------------------------------
    std::fs::write(
        log_dir.join("prompt.txt"),
        &prompt,
    ).ok();

    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        std::fs::write(
            log_dir.join("response.json"),
            pretty,
        ).ok();
    }

    // ------------------------------------------------------------
    // Multi-JSON Support
    //
    // Allow the LLM to return:
    //   1) A single JSON object (previous behavior)
    //   2) An array of JSON objects
    //
    // We merge all `deltas` arrays and concatenate rationales.
    // ------------------------------------------------------------

    let response: InvariantPlanResponse = if payload.is_array() {
        let mut merged_deltas = Vec::new();
        let mut merged_rationale = String::new();

        for item in payload.as_array().unwrap() {
            let parsed: InvariantPlanResponse = serde_json::from_value(item.clone())
                .context("Invalid JSON block inside array response")?;

            merged_deltas.extend(parsed.deltas);

            if !parsed.rationale.is_empty() {
                merged_rationale.push_str(&parsed.rationale);
                merged_rationale.push('\n');
            }
        }

        InvariantPlanResponse {
            deltas: merged_deltas,
            rationale: merged_rationale,
        }
    } else {
        serde_json::from_value(payload)
            .context("LLM payload did not match InvariantPlanResponse schema")?
    };

    // Structural guardrails: return structured rejection instead of aborting tick.
    for delta in &response.deltas {
        if let CodeDelta::ApplyPatch { patch } = delta {
            if patch.contains("*** Update File:") {
                for line in patch.lines() {
                    if let Some(rest) = line.strip_prefix("*** Update File: ") {
                        if !rest.trim().starts_with("src/") {
                            return Err(anyhow::anyhow!(
                                "GUARDRAIL_REJECTION: Patch attempted to modify non-src file: {}",
                                rest
                            ));
                        }
                    }
                }
            }

            // Only reject if an ADDED line introduces the suppressed binding sentinel.
            // Minus lines are removals — those are fine and should not be blocked.
            let adds_suppressed = patch.lines().any(|line| {
                line.starts_with('+') && line.contains("canon suppressed binding")
            });
            if adds_suppressed {
                return Err(anyhow::anyhow!(
                    "GUARDRAIL_REJECTION: Reintroduction of suppressed binding is forbidden. \
                     You must structurally lower the return instead of suppressing it."
                ));
            }
        }
    }

    Ok(response)
}

// ---------------------------------------------------------------------------
// Act
// ---------------------------------------------------------------------------

/// Execute a list of CodeDeltas against the capture directory.
/// ApplyPatch deltas are run via the `apply_patch` tool.
/// Bash deltas are run via sh with the capture dir as cwd.
fn act(deltas: &[CodeDelta], capture_dirs: &[PathBuf]) -> Result<()> {
    let capture_dir = &capture_dirs[0];
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
            CodeDelta::BashReadOnly { command } => {
                let allowed = ["rg", "cat", "ls", "tree", "sed", "awk"];
                let trimmed = command.trim();
                let is_allowed = allowed.iter().any(|a| trimmed.starts_with(a));

                if !is_allowed {
                    anyhow::bail!("Rejected non-whitelisted command: {}", command);
                }

                eprintln!("[invariant] readonly bash: {}", trimmed);

                let status = Command::new("bash")
                    .arg("-c")
                    .arg(trimmed)
                    .current_dir(capture_dir)
                    .status()
                    .context("readonly bash failed to spawn")?;

                anyhow::ensure!(status.success(), "readonly bash exited with {}", status);
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
        "TICK {tick}\n\
__ret gaps remaining: {gaps}\n\
build: {build}\n\
next target: {first_gap}\n\
Respond with ONE fenced ```json block only.",
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

/// Retry path: append a small error addendum to the original prompt.
/// Does NOT re-serialize surface, MIR, or any bootstrap content.
async fn plan_via_llm_retry(
    bridge: &WsBridge,
    url: &str,
    base_prompt: &str,
    error: &str,
    config: &PromptConfig,
    tick: u64,
    log_dir: &Path,
    tab_id_slot: &tokio::sync::Mutex<Option<u32>>,
) -> Result<InvariantPlanResponse> {
    // Only send the addendum — the base_prompt already established context
    let addendum = config.render_retry_addendum(error);
    let prompt = format!("{base_prompt}\n{addendum}");

    let tab_id = get_or_open_tab(bridge, url, tab_id_slot).await?;
    let raw = bridge
        .send_turn(tab_id, prompt.clone())
        .await
        .map_err(|e| anyhow::anyhow!("llm send_turn error: {e}"))?;
    let payload = crate::llm_provider::JsonExtractor::extract(&raw)
        .map_err(|e| anyhow::anyhow!("json extract error: {e}"))?;

    // Persist retry prompt
    std::fs::write(log_dir.join("retry_prompt.txt"), &prompt).ok();
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        std::fs::write(log_dir.join("retry_response.json"), pretty).ok();
    }

    let response: InvariantPlanResponse = serde_json::from_value(payload)
        .context("LLM retry payload did not match InvariantPlanResponse schema")?;

    Ok(response)
}

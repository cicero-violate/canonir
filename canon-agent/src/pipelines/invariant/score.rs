//! Reward scoring for the invariant agent pipeline.
//!
//! R = R_exit + R_compile + R_errors + R_warnings + R_entropy + R_stagnation

/// Parsed summary from `cargo check/build --message-format=json`.
#[derive(Debug, Default, Clone)]
pub struct CargoReport {
    pub error_count:   usize,
    pub warning_count: usize,
}

/// All signals needed for one tick's reward.
#[derive(Debug, Default)]
pub struct RewardSignals {
    /// Exit-check returned 0.
    pub exit_ok:       bool,
    /// Act phase execution failed (patch rejected / bash error).
    pub act_failed:    bool,
    /// Cargo JSON report for this tick (None = no compile ran).
    pub cargo_now:     Option<CargoReport>,
    /// Cargo JSON report from the previous tick (for delta).
    pub cargo_prev:    Option<CargoReport>,
    /// Number of lines changed in all ApplyPatch deltas this tick.
    pub patch_lines:   usize,
    /// Consecutive non-act ticks (stagnation counter snapshot).
    pub stagnation:    usize,
}

/// Breakdown for observability / logging.
#[derive(Debug, Default)]
pub struct RewardBreakdown {
    pub exit:       f32,
    pub compile:    f32,
    pub errors:     f32,
    pub warnings:   f32,
    pub entropy:    f32,
    pub stagnation: f32,
    pub total:      f32,
    pub total_f64:  f64,
}

impl std::fmt::Display for RewardBreakdown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exit={:+.2} compile={:+.2} errors={:+.2} warnings={:+.2} \
             entropy={:+.2} stagnation={:+.2} → total={:+.2}",
            self.exit, self.compile, self.errors, self.warnings,
            self.entropy, self.stagnation, self.total,
        )
    }
}

/// Structured progress metrics exposed to the LLM in prompts (Case 1).
/// These are real structural deltas — not heuristic scalars.
#[derive(Debug, Default, Clone)]
pub struct ProgressMetrics {
    /// Number of `canon suppressed binding` occurrences in exit-check output.
    pub gap_count_now:  usize,
    /// Gap count from the previous tick (0 if unknown).
    pub gap_count_prev: usize,
    /// Whether the last cargo check had zero errors.
    pub compile_ok:     bool,
    /// Number of cargo errors this tick.
    pub compile_errors: usize,
    /// Consecutive non-act ticks.
    pub stagnation:     usize,
}

impl ProgressMetrics {
    /// Count occurrences of the exit-check failure string in raw output.
    pub fn gap_count_from_output(exit_check_output: &str) -> usize {
        exit_check_output
            .lines()
            .filter(|l| l.contains("canon suppressed binding"))
            .count()
    }

    /// Render as a compact block for prompt injection.
    pub fn to_prompt_block(&self) -> String {
        let trend = if self.gap_count_prev == 0 {
            "n/a (first tick)".to_string()
        } else {
            let delta = self.gap_count_prev as i64 - self.gap_count_now as i64;
            match delta.cmp(&0) {
                std::cmp::Ordering::Greater => format!("↓ -{} improved", delta),
                std::cmp::Ordering::Less    => format!("↑ +{} regressed", delta.abs()),
                std::cmp::Ordering::Equal   => "→ unchanged".to_string(),
            }
        };
        format!(
            "gap_count={} (prev={}, trend={})\ncompile={} errors={}\nstagnation={}",
            self.gap_count_now,
            self.gap_count_prev,
            trend,
            if self.compile_ok { "ok" } else { "failing" },
            self.compile_errors,
            self.stagnation,
        )
    }
}

fn clip(v: f32, lo: f32, hi: f32) -> f32 {
    v.clamp(lo, hi)
}

pub fn compute_reward(signals: &RewardSignals) -> RewardBreakdown {
    // --- exit ---
    let r_exit = if signals.exit_ok { 1.0 } else { 0.0 };

    // --- compile ---
    let clean_compile = signals
        .cargo_now
        .as_ref()
        .map(|r| r.error_count == 0)
        .unwrap_or(false);

    let r_compile = if signals.act_failed {
        -0.5
    } else if clean_compile {
        0.4
    } else {
        0.0
    };

    // --- error delta ---
    let r_errors = {
        let prev_e = signals.cargo_prev.as_ref().map(|r| r.error_count).unwrap_or(0) as f32;
        let curr_e = signals.cargo_now.as_ref().map(|r| r.error_count).unwrap_or(0) as f32;
        let delta  = prev_e - curr_e; // positive = improvement
        clip(delta / 10.0, -0.3, 0.3)
    };

    // --- warning delta ---
    let r_warnings = {
        let prev_w = signals.cargo_prev.as_ref().map(|r| r.warning_count).unwrap_or(0) as f32;
        let curr_w = signals.cargo_now.as_ref().map(|r| r.warning_count).unwrap_or(0) as f32;
        let delta  = prev_w - curr_w;
        clip(delta / 20.0, -0.1, 0.1)
    };

    // --- diff entropy ---
    // Normalize by 200 lines (a large focused patch); penalise shotgun diffs.
    let h = (signals.patch_lines as f32 / 200.0).min(1.0);
    let r_entropy = -0.1 * h;

    // --- stagnation ---
    let r_stagnation = if signals.stagnation > 0 {
        -0.05 * signals.stagnation as f32
    } else {
        0.0
    };

    let total = r_exit + r_compile + r_errors + r_warnings + r_entropy + r_stagnation;

    RewardBreakdown {
        exit:       r_exit,
        compile:    r_compile,
        errors:     r_errors,
        warnings:   r_warnings,
        entropy:    r_entropy,
        stagnation: r_stagnation,
        total,
        total_f64:  total as f64,
    }
}

// ---------------------------------------------------------------------------
// Cargo JSON parser
// ---------------------------------------------------------------------------

/// Parse `cargo check/build --message-format=json` output into a `CargoReport`.
/// Each line is a JSON object; we tally `compiler-message` entries by level.
pub fn parse_cargo_json(output: &str) -> CargoReport {
    let mut report = CargoReport::default();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("reason").and_then(|v| v.as_str()) == Some("compiler-message") {
                if let Some(level) = val
                    .pointer("/message/level")
                    .and_then(|v| v.as_str())
                {
                    match level {
                        "error" => report.error_count   += 1,
                        "warning" => report.warning_count += 1,
                        _ => {}
                    }
                }
            }
        }
    }
    report
}

/// Count changed lines (+ and -) across a raw patch string.
pub fn patch_line_count(patch: &str) -> usize {
    patch
        .lines()
        .filter(|l| l.starts_with('+') || l.starts_with('-'))
        .filter(|l| !l.starts_with("+++") && !l.starts_with("---"))
        .count()
}

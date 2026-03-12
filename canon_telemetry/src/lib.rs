//! canon_telemetry — structural surface scanner for emitted canon source.
//!
//! Scans a canon-emitted `src/` directory and accumulates the invariant
//! counters that `run_script.sh` used to compute via shell+rg.

pub mod type_authority;
pub use type_authority::{analyse_capture, write_report as write_type_authority_report, TypeAuthorityReport};
pub mod repair_signal;
pub use repair_signal::{classify as classify_repair_signals, CanonRepairSignal};

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Pattern strings (compiled once via lazy statics)
// ---------------------------------------------------------------------------

macro_rules! pat {
    ($e:expr) => {
        Regex::new($e).expect("hardcoded regex")
    };
}

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// One unresolved `__ret` gap site discovered in the emitted source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetGapSite {
    /// Path relative to the scanned `src/` root.
    pub file: String,
    /// 1-based line number.
    pub line: usize,
    /// The nearest enclosing `fn` signature line, if found.
    pub enclosing_fn: String,
}

/// Structural surface counters for a single emitted canon source tree.
///
/// Corresponds to the shell variables computed in `run_script.sh`:
///
/// ```text
/// S_sup   = suppressed_count
/// S_ret   = suppressed_ret_count
/// S_non   = suppressed_nonret_count  = S_sup - S_ret
/// G_m     = match_gap_count
/// G_c     = call_gap_count
/// G_s     = switch_gap_count
/// G_total = unresolved_gap_total     = S_sup + G_m + G_c + G_s
/// R_gap   = unresolved_ret_gap_count
/// U       = unreachable_count
/// C_m     = match_comment_count
/// C_g     = goto_comment_count
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StructuralSurface {
    pub suppressed_count: usize,
    pub suppressed_ret_count: usize,
    pub suppressed_nonret_count: usize,
    pub match_gap_count: usize,
    pub call_gap_count: usize,
    pub switch_gap_count: usize,
    pub unresolved_gap_total: usize,
    pub unresolved_ret_gap_count: usize,
    pub unreachable_count: usize,
    pub match_comment_count: usize,
    pub goto_comment_count: usize,
    pub ret_gap_sites: Vec<RetGapSite>,
}

impl StructuralSurface {
    /// Print the same human-readable block that `run_script.sh` wrote into
    /// `STRUCTURAL_INVARIANTS_REPORT.md`.
    pub fn print_report(&self) {
        // Intentionally silent (structured data written to disk instead).
    }
}

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

/// Scan the emitted `src/` directory rooted at `src_root` and return the
/// accumulated [`StructuralSurface`].
///
/// Every `.rs` file under `src_root` is read into memory and matched against
/// the same patterns that `run_script.sh` used via `rg`.
pub fn scan(src_root: &Path) -> io::Result<StructuralSurface> {
    // Compile patterns once.
    let re_suppressed = pat!(r"canon suppressed binding");
    let re_suppressed_ret = pat!(r#"__ret\s*=\s*panic!\("canon suppressed binding"\)"#);
    let re_match_gap = pat!(r"canon match result not lowered");
    let re_call_gap = pat!(r"canon call result not lowered");
    let re_switch_gap = pat!(r"canon switch result not lowered");
    let re_ret_gap = pat!(r#"let mut __ret = panic!\("canon (?:suppressed binding|call result not lowered|switch result not lowered|match result not lowered)"\);"#);
    let re_unreachable = pat!(r"unreachable!\(\);");
    let re_match_comment = pat!(r"// match");
    let re_goto_comment = pat!(r"// goto");
    // Pattern to find enclosing fn signatures (mirrors the awk heuristic).
    let re_fn_sig = pat!(r"^\s*(?:pub\s+)?fn\s+");

    let mut surface = StructuralSurface::default();

    for entry in WalkDir::new(src_root).into_iter().filter_map(|e| e.ok()).filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("rs")) {
        let path = entry.path();
        let rel = relative_path(src_root, path);
        let content = std::fs::read_to_string(path)?;
        let lines: Vec<&str> = content.lines().collect();

        for (idx, line) in lines.iter().enumerate() {
            let lineno = idx + 1; // 1-based

            if re_suppressed.is_match(line) {
                surface.suppressed_count += 1;
            }
            if re_suppressed_ret.is_match(line) {
                surface.suppressed_ret_count += 1;
            }
            if re_match_gap.is_match(line) {
                surface.match_gap_count += 1;
            }
            if re_call_gap.is_match(line) {
                surface.call_gap_count += 1;
            }
            if re_switch_gap.is_match(line) {
                surface.switch_gap_count += 1;
            }
            if re_unreachable.is_match(line) {
                surface.unreachable_count += 1;
            }
            if re_match_comment.is_match(line) {
                surface.match_comment_count += 1;
            }
            if re_goto_comment.is_match(line) {
                surface.goto_comment_count += 1;
            }
            if re_ret_gap.is_match(line) {
                surface.unresolved_ret_gap_count += 1;
                let enclosing_fn = find_enclosing_fn(&lines, idx, &re_fn_sig);
                surface.ret_gap_sites.push(RetGapSite { file: rel.clone(), line: lineno, enclosing_fn });
            }
        }
    }

    surface.suppressed_nonret_count = surface.suppressed_count.saturating_sub(surface.suppressed_ret_count);
    surface.unresolved_gap_total = surface.suppressed_count + surface.match_gap_count + surface.call_gap_count + surface.switch_gap_count;

    Ok(surface)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Walk backwards from `line_idx` to find the nearest `fn` signature line.
fn find_enclosing_fn(lines: &[&str], line_idx: usize, re_fn_sig: &Regex) -> String {
    for i in (0..=line_idx).rev() {
        if re_fn_sig.is_match(lines[i]) {
            return lines[i].trim().to_owned();
        }
    }
    "fn <unknown>".to_owned()
}

/// Return a `/`-separated path relative to `root`, falling back to the full
/// path display if stripping fails.
fn relative_path(root: &Path, full: &Path) -> String {
    full.strip_prefix(root).map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")).unwrap_or_else(|_| full.display().to_string())
}

// ---------------------------------------------------------------------------
// Convenience: scan from an emit dir (looks for `src/` inside it)
// ---------------------------------------------------------------------------

/// Like [`scan`] but accepts the emit output directory and appends `src/`
/// automatically, matching the shell script's `emit_src="$ROOT/emit/$fixture/src"`.
///
/// Returns `None` if the `src/` subdirectory does not exist.
pub fn scan_emit_dir(emit_dir: &Path) -> io::Result<Option<StructuralSurface>> {
    let src = emit_dir.join("src");
    if !src.is_dir() {
        return Ok(None);
    }
    scan(&src).map(Some)
}

// ---------------------------------------------------------------------------
// Cargo build — JSON diagnostics
// ---------------------------------------------------------------------------

/// A single compiler diagnostic from `cargo build --message-format json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub level: String,
    pub message: String,
    /// Rust error code extracted from rendered text, e.g. "E0308". Empty string if absent.
    pub error_code: String,
    /// The rendered, human-readable form cargo produces.
    pub rendered: Option<String>,
    /// File + line of the primary span, if present.
    pub primary_span: Option<SpanLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanLocation {
    pub file: String,
    pub line_start: u32,
    pub line_end: u32,
}

/// Result of running `cargo build --message-format json` on an emit dir.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildReport {
    pub success: bool,
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
    /// Error count grouped by Rust error code, e.g. {"E0308": 3, "unknown": 5}.
    pub build_error_categories: HashMap<String, usize>,
    /// First representative rendered snippet (trimmed to 12 lines) per error code.
    /// Gives the agent one concrete example of each category without flooding output.
    pub build_error_samples: HashMap<String, String>,
    /// Error count per emitted source file, e.g. {"src/extractor.rs": 42}.
    /// Derived from primary_span.file; lets the agent prioritize which file to fix first.
    pub errors_by_file: HashMap<String, usize>,
}

impl BuildReport {
    pub fn print_report(&self) {
        // Intentionally silent (JSON report written to disk instead).
    }
}

/// Run `cargo check --message-format json` inside `emit_dir` (offline) and
/// return a [`BuildReport`] parsed from the JSON output.
///
/// `offline` mirrors `CARGO_NET_OFFLINE=true` used in `run_script.sh`.
pub fn build(emit_dir: &Path, offline: bool) -> io::Result<BuildReport> {
    let mut cmd = Command::new("cargo");
    cmd.arg("check").arg("--message-format").arg("json").current_dir(emit_dir);
    if offline {
        cmd.env("CARGO_NET_OFFLINE", "true");
    }

    let output = cmd.output()?;
    let success = output.status.success();
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut report = BuildReport { success, ..Default::default() };

    // Compiled once for extracting Rust error codes from rendered text.
    let re_ecode = Regex::new(r"\[E(\d+)\]").expect("hardcoded regex");
    // Max lines to keep for a sample snippet.
    const SAMPLE_LINES: usize = 12;

    for raw_line in stdout.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let obj: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if obj.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let msg = match obj.get("message") {
            Some(m) => m,
            None => continue,
        };
        let level = msg.get("level").and_then(|l| l.as_str()).unwrap_or("unknown").to_owned();
        let message = msg.get("message").and_then(|m| m.as_str()).unwrap_or("").to_owned();
        let rendered = msg.get("rendered").and_then(|r| r.as_str()).map(|s| s.to_owned());

        // Extract Rust error code from rendered text (e.g. "[E0308]" -> "E0308").
        let error_code = rendered.as_deref().and_then(|r| re_ecode.captures(r)).map(|c| format!("E{}", &c[1])).unwrap_or_default();

        // Extract primary span location.
        let primary_span =
            msg.get("spans").and_then(|s| s.as_array()).and_then(|spans| spans.iter().find(|s| s.get("is_primary").and_then(|p| p.as_bool()).unwrap_or(false))).map(|span| SpanLocation {
                file: span.get("file_name").and_then(|f| f.as_str()).unwrap_or("").to_owned(),
                line_start: span.get("line_start").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
                line_end: span.get("line_end").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
            });

        let diag = Diagnostic { level: level.clone(), message, error_code: error_code.clone(), rendered, primary_span };

        if level == "error" {
            // Category counter.
            let key = if error_code.is_empty() { "unknown".to_owned() } else { error_code.clone() };
            *report.build_error_categories.entry(key.clone()).or_insert(0) += 1;

            // First sample per category (trimmed to SAMPLE_LINES lines).
            report.build_error_samples.entry(key).or_insert_with(|| {
                diag.rendered.as_deref().map(|r| r.trim_end().lines().take(SAMPLE_LINES).collect::<Vec<_>>().join("\n")).unwrap_or_else(|| format!("[{}] {}", diag.level, diag.message))
            });

            // Per-file error count from primary span.
            if let Some(span) = &diag.primary_span {
                if !span.file.is_empty() {
                    *report.errors_by_file.entry(span.file.clone()).or_insert(0) += 1;
                }
            }

            report.errors.push(diag);
        } else if level == "warning" {
            report.warnings.push(diag);
        }
    }

    Ok(report)
}

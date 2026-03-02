//! canon_telemetry — structural surface scanner for emitted canon source.
//!
//! Scans a canon-emitted `src/` directory and accumulates the invariant
//! counters that `run_script.sh` used to compute via shell+rg.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
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
        println!("- emitted structural surface:");
        println!("  - canon suppressed binding count: {}", self.suppressed_count);
        println!("  - canon suppressed __ret count: {}", self.suppressed_ret_count);
        println!("  - canon suppressed non-__ret count: {}", self.suppressed_nonret_count);
        println!("  - canon match gap count: {}", self.match_gap_count);
        println!("  - canon call gap count: {}", self.call_gap_count);
        println!("  - canon switch gap count: {}", self.switch_gap_count);
        println!("  - unresolved gap total: {}", self.unresolved_gap_total);
        println!("  - unresolved __ret gap count: {}", self.unresolved_ret_gap_count);
        println!("  - unreachable count: {}", self.unreachable_count);
        println!("  - // match count: {}", self.match_comment_count);
        println!("  - // goto count: {}", self.goto_comment_count);
        if !self.ret_gap_sites.is_empty() {
            println!("- unresolved __ret gap sites:");
            for site in &self.ret_gap_sites {
                println!("  - {}:{} :: {}", site.file, site.line, site.enclosing_fn);
            }
        }
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
}

impl BuildReport {
    pub fn print_report(&self) {
        println!("- cargo build result: {}", if self.success { "OK" } else { "FAILED" });
        println!("  - error count: {}", self.errors.len());
        println!("  - warning count: {}", self.warnings.len());
        if !self.errors.is_empty() {
            println!("- cargo errors:");
            for d in &self.errors {
                if let Some(r) = &d.rendered {
                    // Print rendered text indented, trimming trailing newline.
                    for line in r.trim_end().lines() {
                        println!("  {}", line);
                    }
                } else {
                    println!("  [{}] {}", d.level, d.message);
                }
            }
        }
    }
}

/// Run `cargo build --message-format json` inside `emit_dir` (offline) and
/// return a [`BuildReport`] parsed from the JSON output.
///
/// `offline` mirrors `CARGO_NET_OFFLINE=true` used in `run_script.sh`.
pub fn build(emit_dir: &Path, offline: bool) -> io::Result<BuildReport> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("--message-format").arg("json").current_dir(emit_dir);
    if offline {
        cmd.env("CARGO_NET_OFFLINE", "true");
    }

    let output = cmd.output()?;
    // cargo --message-format json writes diagnostics to stdout; exit code
    // reflects success/failure.
    let success = output.status.success();
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut report = BuildReport { success, ..Default::default() };

    for raw_line in stdout.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let obj: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Only care about compiler-message lines.
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

        // Extract primary span location.
        let primary_span =
            msg.get("spans").and_then(|s| s.as_array()).and_then(|spans| spans.iter().find(|s| s.get("is_primary").and_then(|p| p.as_bool()).unwrap_or(false))).map(|span| SpanLocation {
                file: span.get("file_name").and_then(|f| f.as_str()).unwrap_or("").to_owned(),
                line_start: span.get("line_start").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
                line_end: span.get("line_end").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
            });

        let diag = Diagnostic { level: level.clone(), message, rendered, primary_span };
        match level.as_str() {
            "error" => report.errors.push(diag),
            "warning" => report.warnings.push(diag),
            _ => {}
        }
    }

    Ok(report)
}

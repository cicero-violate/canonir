//! rename — CLI tool for renaming symbols in a Rust project.
//!
//! Usage:
//!   rename --project <path> --old <symbol> --new <symbol>   # rename one symbol
//!   rename --project <path> --pairs <old>=<new>[,...]        # rename multiple at once
//!   rename --project <path> --list                           # list all known symbols
//!   rename --project <path> --preview --old <s> --new <s>   # show diff without writing
//!   rename --project <path> --json --old <s> --new <s>      # JSON report to stdout
//!
//! Flags:
//!   --dry-run   Validate and preview but do not write files
//!   --json      Output result as JSON to stdout

use anyhow::{anyhow, Result};
use canon_editor::{rename_symbol_pairs, RenameRunReport};
use canon_editor::symbol_index::SymbolIndex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let project = arg_value(&args, "--project")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("--project <path> is required"))?;

    if !project.exists() {
        return Err(anyhow!("project path does not exist: {}", project.display()));
    }

    let emit_json = args.iter().any(|a| a == "--json");
    let dry_run   = args.iter().any(|a| a == "--dry-run");
    let do_list   = args.iter().any(|a| a == "--list");
    let do_preview= args.iter().any(|a| a == "--preview");

    // --list: print all known symbol IDs and exit
    if do_list {
        return cmd_list(&project, emit_json);
    }

    // Build rename pairs
    let pairs = build_pairs(&args)?;
    if pairs.is_empty() {
        return Err(anyhow!(
            "no rename pairs given; use --old <sym> --new <sym> or --pairs old=new[,old2=new2,...]"
        ));
    }

    if do_preview || dry_run {
        return cmd_preview(&project, &pairs, emit_json);
    }

    cmd_rename(&project, &pairs, emit_json)
}

// ── Commands ────────────────────────────────────────────────────────────────

fn cmd_list(project: &Path, emit_json: bool) -> Result<()> {
    let session = SymbolIndex::build(project)
        .map_err(|e| anyhow!("failed to index {}: {e}", project.display()))?;
    let catalog = session.symbol_catalog();
    if emit_json {
        let items: Vec<serde_json::Value> = catalog
            .iter()
            .map(|(id, kind)| serde_json::json!({ "id": id, "kind": kind }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        eprintln!("[rename] {} symbols in {}", catalog.len(), project.display());
        for (id, kind) in &catalog {
            println!("{kind:12} {id}");
        }
    }
    Ok(())
}

fn cmd_preview(project: &Path, pairs: &[(String, String)], emit_json: bool) -> Result<()> {
    use canon_editor::edit::ProjectEditor;
    use canon_editor::structured::FieldMutation;

    let session = Arc::new(
        SymbolIndex::build(project)
            .map_err(|e| anyhow!("failed to index {}: {e}", project.display()))?,
    );

    let mut editor = ProjectEditor::load_with_session(project, Arc::clone(&session))
        .map_err(|e| anyhow!("failed to load editor: {e}"))?;

    for (old, new) in pairs {
        editor
            .queue_by_id(old, FieldMutation::RenameIdent(new.clone()))
            .map_err(|e| anyhow!("cannot queue rename {old} → {new}: {e}"))?;
    }

    let conflicts = editor.validate().map_err(|e| anyhow!("validation failed: {e}"))?;
    if !conflicts.is_empty() {
        for c in &conflicts {
            eprintln!("conflict: {} — {}", c.symbol_id, c.reason);
        }
        return Err(anyhow!("{} conflict(s) prevent rename", conflicts.len()));
    }

    let preview = editor.preview().map_err(|e| anyhow!("preview failed: {e}"))?;

    if emit_json {
        let pairs_v: Vec<serde_json::Value> = pairs
            .iter()
            .map(|(o, n)| serde_json::json!({ "old": o, "new": n }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "ok",
                "dry_run": true,
                "pairs": pairs_v,
                "diff": preview,
            }))?
        );
    } else {
        println!("{preview}");
    }
    Ok(())
}

fn cmd_rename(project: &Path, pairs: &[(String, String)], emit_json: bool) -> Result<()> {
    let report = rename_symbol_pairs(project, pairs);
    print_report(&report, pairs, emit_json, project)
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Build rename pairs from CLI flags.
/// Accepts:
///   --old <sym> --new <sym>         (single pair)
///   --pairs old1=new1,old2=new2     (multiple pairs, comma-separated)
///   --pairs old1=new1 --pairs ...   (repeated flag)
fn build_pairs(args: &[String]) -> Result<Vec<(String, String)>> {
    let mut pairs: Vec<(String, String)> = Vec::new();

    // --old / --new (single pair)
    if let (Some(old), Some(new)) = (arg_value(args, "--old"), arg_value(args, "--new")) {
        pairs.push((old, new));
    }

    // --pairs old=new[,old2=new2,...]  (may appear multiple times)
    for i in 0..args.len().saturating_sub(1) {
        if args[i] != "--pairs" {
            continue;
        }
        let val = &args[i + 1];
        for item in val.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            let (old, new) = item
                .split_once('=')
                .ok_or_else(|| anyhow!("--pairs entry '{item}' is not in old=new format"))?;
            pairs.push((old.trim().to_string(), new.trim().to_string()));
        }
    }

    Ok(pairs)
}

fn print_report(
    report: &RenameRunReport,
    pairs: &[(String, String)],
    emit_json: bool,
    project: &Path,
) -> Result<()> {
    if emit_json {
        let pairs_v: Vec<serde_json::Value> = pairs
            .iter()
            .map(|(o, n)| serde_json::json!({ "old": o, "new": n }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": report.status(),
                "project": project.display().to_string(),
                "pairs": pairs_v,
                "def_paths": report.def_paths,
                "error": report.error,
            }))?
        );
    } else {
        if let Some(err) = &report.error {
            eprintln!("rename failed: {err}");
            return Err(anyhow!("rename failed"));
        }
        let n = pairs.len();
        eprintln!(
            "[rename] {} pair(s) applied in {}",
            n,
            project.display()
        );
        for (old, new) in pairs {
            println!("{old} → {new}");
        }
        for path in &report.def_paths {
            println!("  wrote {path}");
        }
    }
    Ok(())
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

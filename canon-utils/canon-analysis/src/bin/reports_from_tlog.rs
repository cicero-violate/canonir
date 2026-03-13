// DEPRECATED CLI
// reports_from_tlog performs a full rebuild from the event log.
// Production runtime uses event-driven consumers instead.

use std::env;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use canon_analysis::generate_reports_from_tlog;
use canon_analysis::panic_capture::install_panic_hook;
use canon_types::ReportLayout;

fn main() -> Result<()> {
    std::env::set_var("RUST_BACKTRACE", "full");
    let mut tlog: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut debug_panic_cfg = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tlog" => {
                let value = args.next().ok_or_else(|| anyhow!("--tlog requires a path"))?;
                tlog = Some(PathBuf::from(value));
            }
            "--out" => {
                let value = args.next().ok_or_else(|| anyhow!("--out requires a path"))?;
                out_dir = Some(PathBuf::from(value));
            }
            "--debug-panic-cfg" => {
                debug_panic_cfg = true;
            }
            _ => return Err(anyhow!("unknown argument: {}", arg)),
        }
    }

    let tlog = tlog.ok_or_else(|| anyhow!("--tlog is required"))?;
    let out_dir = out_dir.ok_or_else(|| anyhow!("--out is required"))?;
    if tlog.is_dir() {
        eprintln!(
            "[reports] binary tlog directory detected: {}",
            tlog.display()
        );
    }
    let layout = ReportLayout::from_crate_root(out_dir.clone());
    let panic_log = layout.analysis_dir().join("panic_records.jsonl");
    install_panic_hook(panic_log.to_string_lossy().as_ref());
    if debug_panic_cfg {
        panic!("debug cfg inspection");
    }
    generate_reports_from_tlog(&tlog, &out_dir)
}

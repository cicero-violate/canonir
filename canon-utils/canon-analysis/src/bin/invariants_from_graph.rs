// DEPRECATED CLI
// invariant generation now occurs incrementally through event consumers.

use std::env;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use canon_analysis::run_invariant_pipeline;
use canon_types::ReportLayout;

fn main() -> Result<()> {
    let mut graph_dir: Option<PathBuf> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--graph" => {
                let value = args.next().ok_or_else(|| anyhow!("--graph requires a path"))?;
                graph_dir = Some(PathBuf::from(value));
            }
            _ => return Err(anyhow!("unknown argument: {}", arg)),
        }
    }

    let graph_dir = graph_dir.ok_or_else(|| anyhow!("--graph is required"))?;
    let layout = ReportLayout::from_crate_root(
        graph_dir
            .parent()
            .unwrap_or(&graph_dir)
            .to_path_buf(),
    );
    run_invariant_pipeline(
        &graph_dir,
        &layout.invariants_dir(),
        &layout.meta_dir(),
        &layout.analysis_dir(),
        &layout.metrics_dir(),
    )
}

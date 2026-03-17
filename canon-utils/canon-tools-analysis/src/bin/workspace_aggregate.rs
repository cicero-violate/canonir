use anyhow::{anyhow, Result};
use std::env;
use std::path::PathBuf;

use canon_analysis::aggregate_workspace;

fn main() -> Result<()> {
    let mut root: Option<PathBuf> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--reports-root" => {
                let value = args.next().ok_or_else(|| anyhow!("--reports-root requires a path"))?;
                root = Some(PathBuf::from(value));
            }
            _ => return Err(anyhow!("unknown argument: {}", arg)),
        }
    }
    let root = root.unwrap_or_else(|| PathBuf::from("state/reports_out"));
    aggregate_workspace(&root)
}

use std::env;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use canon_reports::generate_reports_from_tlog;

fn main() -> Result<()> {
    let mut tlog: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
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
            _ => return Err(anyhow!("unknown argument: {}", arg)),
        }
    }

    let tlog = tlog.ok_or_else(|| anyhow!("--tlog is required"))?;
    let out_dir = out_dir.ok_or_else(|| anyhow!("--out is required"))?;
    generate_reports_from_tlog(&tlog, &out_dir)
}

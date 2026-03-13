use anyhow::{anyhow, Result};
use canon_tlog_writer::{BinarySegmentWriter, CanonEvent};
use canon_types::KernelEvent;
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut tlog_dir: Option<PathBuf> = None;
    let mut include_session = true;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--tlog" => {
                i += 1;
                tlog_dir = args.get(i).map(PathBuf::from);
            }
            "--no-session" => {
                include_session = false;
            }
            _ => {}
        }
        i += 1;
    }

    let tlog_dir = tlog_dir.ok_or_else(|| anyhow!("missing --tlog <dir>"))?;
    let writer = BinarySegmentWriter::open(&tlog_dir)?;

    if include_session {
        let session = KernelEvent::SessionStart {
            project: "manual".to_string(),
            schema: 2,
            byte_offset: 0,
        };
        let val = serde_json::to_value(&session)?;
        let canon = CanonEvent::new("kernel", "kernel_event", val);
        let _ = writer.append_event(&canon);
    }

    let node = KernelEvent::NodeDefined {
        symbol: "manual::module".to_string(),
        kind: "MODULE".to_string(),
        file: "src/lib.rs".to_string(),
        line: 1,
        col: 1,
        lo: 0,
        hi: 0,
    };
    let val = serde_json::to_value(&node)?;
    let canon = CanonEvent::new("kernel", "kernel_event", val);
    let _ = writer.append_event(&canon);

    Ok(())
}

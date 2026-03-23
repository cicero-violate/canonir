use anyhow::{bail, Result};
use crate::event_stream::event_replay::{replay_tlog, DispatchMode};
use crate::rustc::state::RustcState;
use std::path::Path;

pub fn verify_replay(original: &RustcState, tlog_path: &Path) -> Result<()> {
    let mut replayed = original.clone();
    replayed.tick = 0;
    replayed.last_event_id = 0;
    replayed.known_symbols.clear();
    replayed.known_edges.clear();
    replayed.known_files.clear();

    replay_tlog(&mut replayed, tlog_path, DispatchMode::Batch)?;

    if &replayed != original {
        bail!("replay verification failed: replayed state differs from original");
    }
    if replayed.removed_symbols != original.removed_symbols {
        bail!("replay verification failed: removed_symbols mismatch");
    }
    if replayed.removed_edges != original.removed_edges {
        bail!("replay verification failed: removed_edges mismatch");
    }
    Ok(())
}

use anyhow::Result;
use canon_runtime_supervisor::binary_supervisor::run_binary_supervisor;
use std::path::Path;

fn main() -> Result<()> {
    let binary = Path::new("/workspace/ai_sandbox/canon/target/debug/canon-runtime");
    run_binary_supervisor(binary);
    Ok(())
}

use anyhow::Result;
use canon_runtime_supervisor::binary_supervisor::run_binary_supervisor;
use std::path::Path;

fn main() -> Result<()> {
    // LLM worker must run only in runtime process
    // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
    eprintln!("[ENTER ROOT] {}:{} {} - supervisor::main", file!(), line!(), module_path!());
    let binary = Path::new("/workspace/ai_sandbox/canon/target/debug/canon-runtime");
    run_binary_supervisor(binary);
    Ok(())
}

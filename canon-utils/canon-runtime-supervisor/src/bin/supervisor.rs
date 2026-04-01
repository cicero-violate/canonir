use anyhow::Result;
use canon_runtime_supervisor::binary_supervisor::run_binary_supervisor;
use std::path::Path;

fn main() -> Result<()> {
    canon_exec::init_llm_worker();
    eprintln!("[LLM INIT] init_llm_worker called in supervisor (parent process)");
    #[cfg(feature = "trace")]
    eprintln!("[TRACE] {}:{} {} - entering main", file!(), line!(), module_path!());
    let binary = Path::new("/workspace/ai_sandbox/canon/target/debug/canon-runtime");
    run_binary_supervisor(binary);
    Ok(())
}

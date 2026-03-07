use canon_agent_v2::ir::{CanonicalMeta, Language, Project, VersionContract, Word};
use canon_agent_v2::layout::FileTopology;
use canon_agent_v2::ws_server;
use std::env;
use std::path::PathBuf;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let usage = || {
        println!("Usage:");
        println!("  canon-agent run-capability <cwd> [max_ticks=0]");
    };
    if args.len() < 2 {
        usage();
        return Ok(());
    }
    match args[1].as_str() {
        "run-capability" => {
            if args.len() < 3 {
                usage();
                return Ok(());
            }
            let cwd_root = PathBuf::from(&args[2]);
            let cwd: Vec<PathBuf> = vec![cwd_root.clone()];
            let capture_dir = cwd_root
                .join("test_projects/test_rust_projects/capture/repomap");
            let emit_dir = cwd_root
                .join("test_projects/test_rust_projects/emit/repomap");
            let orchestration_bin = cwd_root.join("target/debug/orchestration");
            let max_ticks: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            let addr = "127.0.0.1:9100".parse()?;
            let cap_config = canon_agent_v2::pipelines::capability::config::CapabilityConfig::snapshot_store_load()?;
            let bridge = ws_server::spawn(addr, cap_config.response_timeout_secs);
            let pipeline = canon_agent_v2::pipelines::capability::CapabilityPipeline::new(
                bridge,
            );
            let mut ir = canon_agent_v2::ir::SystemState::new(
                CanonicalMeta {
                    version: "0.1.0".into(),
                    law_revision: Word::new("genesis").expect("valid word"),
                    description: "capability pipeline stub".into(),
                },
                VersionContract {
                    current: "0.1.0".into(),
                    compatible_with: vec![],
                    migration_proofs: vec![],
                },
                Project {
                    name: Word::new("canon_agent").expect("valid word"),
                    version: "0.1.0".into(),
                    language: Language::Rust,
                },
            );
            let mut layout = FileTopology::default();
            let ctx = canon_agent_v2::pipelines::PipelineContext {
                cwd: cwd.clone(),
                capture_dir: capture_dir.clone(),
                emit_dir: emit_dir.clone(),
                orchestration_bin: orchestration_bin.clone(),
                workspace: cwd[0].clone(),
                tick: 0,
            };
            let loop_config = canon_agent_v2::runtime::agent_loop::AgentLoopConfig {
                max_ticks,
                ..Default::default()
            };
            canon_agent_v2::runtime::agent_loop::run_agent_loop(
                    &pipeline,
                    &ctx,
                    &mut ir,
                    &mut layout,
                    loop_config,
                )
                .await?;
        }
        _ => {
            usage();
        }
    }
    Ok(())
}

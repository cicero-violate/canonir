use canon_agent::agent_commands::{execute_agent_command, AgentCommand};
use canon_agent::agent_config::AgentConfig;
use canon_agent::bootstrap::{seed_capability_graph, seed_refactor_proposal};
use canon_agent::call::AgentCallOutput;
use canon_agent::ir::SystemState;
use canon_agent::layout::FileTopology;
use canon_agent::pipelines::refactor::RefactorProposal;
use canon_agent::runner::{run_agent, RunnerConfig};
use canon_agent::ws_server;
use std::env;
use std::fs;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    let usage = || {
        println!("Usage:");
        println!("  canon-agent show-ledger <ledger.json>");
        println!("  canon-agent show-graph <graph.json>");
        println!("  canon-agent run-pipeline <ir.json> <layout.json> <proposal.json> <outputs.json>");
        println!("  canon-agent run-agent <ir.json> <layout.json> <graph.json> <workspace>");
        println!("  canon-agent run-multi-dag <cwd> [max_ticks=20]");
        println!("  canon-agent run-capability <cwd> [max_ticks=0]");
    };

    if args.len() < 2 {
        usage();
        return Ok(());
    }

    match args[1].as_str() {
        "show-ledger" => {
            if args.len() != 3 {
                usage();
                return Ok(());
            }
            execute_agent_command(AgentCommand::ShowLedger { ledger_path: args[2].clone() }).await?;
        }

        "show-graph" => {
            if args.len() != 3 {
                usage();
                return Ok(());
            }
            execute_agent_command(AgentCommand::ShowGraph { graph_path: args[2].clone() }).await?;
        }

        "run-pipeline" => {
            if args.len() != 6 {
                usage();
                return Ok(());
            }
            let ir: SystemState = serde_json::from_slice(&fs::read(&args[2])?)?;
            let layout: FileTopology = serde_json::from_slice(&fs::read(&args[3])?)?;
            let proposal: RefactorProposal = serde_json::from_slice(&fs::read(&args[4])?)?;
            let outputs: Vec<AgentCallOutput> = serde_json::from_slice(&fs::read(&args[5])?)?;
            execute_agent_command(AgentCommand::RunPipeline { ir, layout, proposal, stage_outputs: outputs }).await?;
        }

        "run-agent" => {
            if args.len() != 6 {
                usage();
                return Ok(());
            }
            let ir_path = PathBuf::from(&args[2]);
            let layout_path = PathBuf::from(&args[3]);
            let graph_path = PathBuf::from(&args[4]);
            let workspace = PathBuf::from(&args[5]);

            let mut ir: SystemState = serde_json::from_slice(&fs::read(&ir_path)?)?;
            let mut layout: FileTopology = serde_json::from_slice(&fs::read(&layout_path)?)?;
            let mut graph = if graph_path.exists() {
                canon_agent::io::load_capability_graph(&graph_path)?
            } else {
                eprintln!("[main] no graph file found — seeding default 5-node graph");
                seed_capability_graph()
            };

            let target_module = ir.modules.first().map(|m| m.id.clone()).unwrap_or_else(|| "core".to_string());
            let proposal = seed_refactor_proposal(&target_module);

            let agent_cfg = AgentConfig::load(&workspace).map_err(|e| {
                eprintln!("[main] fatal: {e}");
                e
            })?;

            eprintln!("[main] chatgpt_url : {}", agent_cfg.chatgpt_url);

            let config = RunnerConfig {
                max_ticks: agent_cfg.max_ticks.unwrap_or(0),
                chatgpt_url: agent_cfg.chatgpt_url.clone(),
                meta_tick_interval: agent_cfg.meta_tick_interval.unwrap_or(10),
                policy_update_interval: agent_cfg.policy_update_interval.unwrap_or(5),
                ledger_alpha: 0.1,
                base_trust_threshold: 0.5,
                graph_out: workspace.join("graph.json"),
                ledger_out: workspace.join("ledger.json"),
                ir_out: ir_path.clone(),
                workspace: workspace.clone(),
            };

            let addr = "127.0.0.1:9100".parse()?;
            let bridge = ws_server::spawn(addr, 60);

            eprintln!("[main] starting run-agent loop");
            eprintln!("[main] workspace : {}", workspace.display());
            eprintln!("[main] ir        : {}", ir_path.display());
            eprintln!("[main] graph     : {}", graph_path.display());

            let stats = run_agent(&mut ir, &mut layout, &mut graph, proposal, &config, &bridge).await?;
            eprintln!("[main] run-agent complete — {} ticks", stats.len());
            for s in &stats {
                eprintln!(
                    "  tick {:>4}  nodes={} llm_err={} reward={} meta={} policy={}",
                    s.tick_number,
                    s.nodes_called,
                    s.llm_errors,
                    s.pipeline_reward.map(|r| format!("{:.4}", r)).unwrap_or_else(|| "-".into()),
                    s.meta_tick_fired,
                    s.policy_updated,
                );
            }
        }

        "run-multi-dag" => {
            if args.len() < 3 {
                usage();
                return Ok(());
            }
            let cwd_root = PathBuf::from(&args[2]);
            let cwd: Vec<PathBuf> = vec![cwd_root.clone()];
            let capture_dir = cwd_root.join("test_projects/test_rust_projects/capture/repomap");
            let emit_dir = cwd_root.join("test_projects/test_rust_projects/emit/repomap");
            let orchestration_bin = cwd_root.join("target/debug/orchestration");
            let max_ticks: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);

            let addr = "127.0.0.1:9100".parse()?;
            let config = canon_agent::pipelines::capability::config::CapabilityConfig::load()?;
            let bridge = ws_server::spawn(addr, config.response_timeout_secs);

            let pipeline = canon_agent::pipelines::capability::CapabilityPipeline::new(bridge);

            // InvariantPipeline operates purely on files — ir and layout are unused.
            let mut ir = SystemState::new(
                CanonicalMeta { version: "0.1.0".into(), law_revision: Word::new("genesis").expect("valid word"), description: "multi-dag pipeline stub".into() },
                VersionContract { current: "0.1.0".into(), compatible_with: vec![], migration_proofs: vec![] },
                Project { name: Word::new("canon_agent").expect("valid word"), version: "0.1.0".into(), language: Language::Rust },
            );
            let mut layout = FileTopology::default();

            use canon_agent::pipelines::Pipeline;
            for tick in 1..=max_ticks {
                let ctx = canon_agent::pipelines::PipelineContext {
                    cwd: cwd.clone(),
                    capture_dir: capture_dir.clone(),
                    emit_dir: emit_dir.clone(),
                    orchestration_bin: orchestration_bin.clone(),
                    workspace: cwd[0].clone(),
                    tick,
                };

                let outcome = pipeline.run_tick(&ctx, &mut ir, &mut layout).await?;
                eprintln!("[main] tick {tick} done — {}", outcome.summary);
                eprintln!("[main] reward={:.4} advanced={}", outcome.reward, outcome.advanced);

                if outcome.advanced {
                    eprintln!("[main] exit check passed — stopping");
                    break;
                }
            }
        }

        "run-capability" => {
            if args.len() < 3 {
                usage();
                return Ok(());
            }
            let cwd_root = PathBuf::from(&args[2]);
            let cwd: Vec<PathBuf> = vec![cwd_root.clone()];
            let capture_dir = cwd_root.join("test_projects/test_rust_projects/capture/repomap");
            let emit_dir = cwd_root.join("test_projects/test_rust_projects/emit/repomap");
            let orchestration_bin = cwd_root.join("target/debug/orchestration");
            let max_ticks: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);

            let addr = "127.0.0.1:9100".parse()?;
            let cap_config = canon_agent::pipelines::capability::config::CapabilityConfig::load()?;
            let bridge = ws_server::spawn(addr, cap_config.response_timeout_secs);

            let pipeline = canon_agent::pipelines::capability::CapabilityPipeline::new(bridge);

            let mut ir = SystemState::new(
                CanonicalMeta { version: "0.1.0".into(), law_revision: Word::new("genesis").expect("valid word"), description: "capability pipeline stub".into() },
                VersionContract { current: "0.1.0".into(), compatible_with: vec![], migration_proofs: vec![] },
                Project { name: Word::new("canon_agent").expect("valid word"), version: "0.1.0".into(), language: Language::Rust },
            );
            let mut layout = FileTopology::default();

            let ctx = canon_agent::pipelines::PipelineContext {
                cwd: cwd.clone(),
                capture_dir: capture_dir.clone(),
                emit_dir: emit_dir.clone(),
                orchestration_bin: orchestration_bin.clone(),
                workspace: cwd[0].clone(),
                tick: 0,
            };
            let loop_config = canon_agent::runtime::agent_loop::AgentLoopConfig {
                max_ticks,
                ..Default::default()
            };
            canon_agent::runtime::agent_loop::run_agent_loop(
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
use canon_agent::ir::{CanonicalMeta, Language, Project, VersionContract, Word};

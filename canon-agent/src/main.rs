use canon_agent::agent_commands::{execute_agent_command, AgentCommand};
use canon_agent::bootstrap::{seed_capability_graph, seed_refactor_proposal};
use canon_agent::ir::SystemState;
use canon_agent::layout::FileTopology;
use canon_agent::runner::{run_agent, RunnerConfig};
use canon_agent::ws_server;
use canon_agent::call::AgentCallOutput;
use canon_agent::pipelines::refactor::RefactorProposal;
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
        println!("  canon-agent run-invariant <capture_dir> <emit_dir> <orchestration_bin>");
    };

    if args.len() < 2 {
        usage();
        return Ok(());
    }

    match args[1].as_str() {
        "show-ledger" => {
            if args.len() != 3 { usage(); return Ok(()); }
            execute_agent_command(AgentCommand::ShowLedger {
                ledger_path: args[2].clone(),
            }).await?;
        }

        "show-graph" => {
            if args.len() != 3 { usage(); return Ok(()); }
            execute_agent_command(AgentCommand::ShowGraph {
                graph_path: args[2].clone(),
            }).await?;
        }

        "run-pipeline" => {
            if args.len() != 6 { usage(); return Ok(()); }
            let ir: SystemState            = serde_json::from_slice(&fs::read(&args[2])?)?;
            let layout: FileTopology       = serde_json::from_slice(&fs::read(&args[3])?)?;
            let proposal: RefactorProposal = serde_json::from_slice(&fs::read(&args[4])?)?;
            let outputs: Vec<AgentCallOutput> = serde_json::from_slice(&fs::read(&args[5])?)?;
            execute_agent_command(AgentCommand::RunPipeline {
                ir,
                layout,
                proposal,
                stage_outputs: outputs,
            }).await?;
        }

        "run-agent" => {
            if args.len() != 6 { usage(); return Ok(()); }
            let ir_path     = PathBuf::from(&args[2]);
            let layout_path = PathBuf::from(&args[3]);
            let graph_path  = PathBuf::from(&args[4]);
            let workspace   = PathBuf::from(&args[5]);

            let mut ir: SystemState      = serde_json::from_slice(&fs::read(&ir_path)?)?;
            let mut layout: FileTopology = serde_json::from_slice(&fs::read(&layout_path)?)?;
            let mut graph = if graph_path.exists() {
                canon_agent::io::load_capability_graph(&graph_path)?
            } else {
                eprintln!("[main] no graph file found — seeding default 5-node graph");
                seed_capability_graph()
            };

            let target_module = ir.modules.first()
                .map(|m| m.id.clone())
                .unwrap_or_else(|| "core".to_string());
            let proposal = seed_refactor_proposal(&target_module);

            let config = RunnerConfig {
                max_ticks: 0,
                meta_tick_interval: 10,
                policy_update_interval: 5,
                ledger_alpha: 0.1,
                base_trust_threshold: 0.5,
                graph_out:  workspace.join("graph.json"),
                ledger_out: workspace.join("ledger.json"),
                ir_out:     ir_path.clone(),
                workspace:  workspace.clone(),
            };

            let addr = "127.0.0.1:9100".parse()?;
            let bridge = ws_server::spawn(addr);

            eprintln!("[main] starting run-agent loop");
            eprintln!("[main] workspace : {}", workspace.display());
            eprintln!("[main] ir        : {}", ir_path.display());
            eprintln!("[main] graph     : {}", graph_path.display());

            let stats = run_agent(&mut ir, &mut layout, &mut graph, proposal, &config, &bridge).await?;
            eprintln!("[main] run-agent complete — {} ticks", stats.len());
            for s in &stats {
                eprintln!(
                    "  tick {:>4}  nodes={} llm_err={} reward={} meta={} policy={}",
                    s.tick_number, s.nodes_called, s.llm_errors,
                    s.pipeline_reward.map(|r| format!("{:.4}", r)).unwrap_or_else(|| "-".into()),
                    s.meta_tick_fired, s.policy_updated,
                );
            }
        }

        "run-invariant" => {
            if args.len() != 5 { usage(); return Ok(()); }
            let capture_dir       = PathBuf::from(&args[2]);
            let emit_dir          = PathBuf::from(&args[3]);
            let orchestration_bin = PathBuf::from(&args[4]);

            let addr = "127.0.0.1:9100".parse()?;
            let bridge = ws_server::spawn(addr);

            let pipeline = canon_agent::pipelines::invariant::InvariantPipeline { bridge };
            let ctx = canon_agent::pipelines::PipelineContext {
                capture_dir,
                emit_dir,
                orchestration_bin,
                workspace: PathBuf::from("."),
                tick: 1,
            };

            // InvariantPipeline operates purely on files — ir and layout are unused.
            let mut ir = SystemState::new(
                CanonicalMeta {
                    version: "0.1.0".into(),
                    law_revision: Word::new("genesis").expect("valid word"),
                    description: "invariant pipeline stub".into(),
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

            use canon_agent::pipelines::Pipeline;
            let outcome = pipeline.run_tick(&ctx, &mut ir, &mut layout).await?;
            eprintln!("[main] invariant tick done — {}", outcome.summary);
            eprintln!("[main] reward={:.4} advanced={}", outcome.reward, outcome.advanced);
        }

        _ => {
            usage();
        }
    }

    Ok(())
}
use canon_agent::ir::{CanonicalMeta, VersionContract, Project, Language, Word};

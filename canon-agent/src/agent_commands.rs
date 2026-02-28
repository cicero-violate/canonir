use crate::call::AgentCallOutput;
use crate::io::{load_capability_graph, save_capability_graph};
use crate::pipeline::run_refactor_pipeline;
use crate::refactor::RefactorProposal;
use crate::reward::NodeRewardLedger;
use crate::layout::FileTopology;
use crate::ir::SystemState;

use std::error::Error;
use std::fs;
use std::path::Path;

/// Minimal orchestration surface for canon-agent.
/// This intentionally excludes kernel, GPU, ingest,
/// DSL auto-application, and other monolith features.

pub enum AgentCommand {
    RunPipeline {
        ir: SystemState,
        layout: FileTopology,
        proposal: RefactorProposal,
        stage_outputs: Vec<AgentCallOutput>,
    },
    MetaTick {
        graph_path: String,
        ledger_path: String,
        output_graph: String,
    },
    ShowLedger {
        ledger_path: String,
    },
    ShowGraph {
        graph_path: String,
    },
}

pub async fn execute_agent_command(cmd: AgentCommand) -> Result<(), Box<dyn Error>> {
    match cmd {
        AgentCommand::RunPipeline {
            ir,
            layout,
            proposal,
            stage_outputs,
        } => {
            let result = run_refactor_pipeline(
                &ir,
                &layout,
                proposal,
                &stage_outputs,
            )?;

            println!(
                "Pipeline OK — reward={:.4}  admission={}",
                result.reward, result.admission_id
            );
        }

        AgentCommand::MetaTick {
            graph_path,
            ledger_path,
            output_graph,
        } => {
            let cap_graph = load_capability_graph(Path::new(&graph_path))?;
            let ledger_doc: NodeRewardLedger =
                serde_json::from_slice(&fs::read(&ledger_path)?)?;

            let result = crate::meta::evolve_capability_graph(&cap_graph, &ledger_doc)?;

            println!("Meta-tick OK");
            println!(
                "  entropy: {:.4} → {:.4}",
                result.entropy_before, result.entropy_after
            );
            println!("  applied mutations : {}", result.applied.len());
            println!("  rejected mutations: {}", result.rejected.len());

            save_capability_graph(&result.graph, Path::new(&output_graph))?;
        }

        AgentCommand::ShowLedger { ledger_path } => {
            let ledger_doc: NodeRewardLedger =
                serde_json::from_slice(&fs::read(&ledger_path)?)?;

            let ranked = ledger_doc.ranked_nodes();

            println!("{:<30} {:>10} {:>10}", "node_id", "ema_reward", "run_count");
            println!("{}", "-".repeat(54));

            for entry in ranked {
                println!(
                    "{:<30} {:>10.4} {:>10}",
                    entry.node_id, entry.ema_reward, entry.run_count
                );
            }

            println!("aggregate reward: {:.4}", ledger_doc.aggregate_reward());
        }

        AgentCommand::ShowGraph { graph_path } => {
            let cap_graph = load_capability_graph(Path::new(&graph_path))?;

            println!(
                "Capability graph: {} nodes, {} edges",
                cap_graph.nodes.len(),
                cap_graph.edges.len()
            );
            println!("Entropy H(G) = {:.4}", cap_graph.entropy());

            println!("{:<20} {:<12} {}", "id", "kind", "label");
            println!("{}", "-".repeat(60));

            for node in &cap_graph.nodes {
                println!(
                    "{:<20} {:<12} {}",
                    node.id,
                    format!("{:?}", node.kind),
                    node.label
                );
            }
        }
    }

    Ok(())
}

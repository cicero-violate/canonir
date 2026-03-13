{
"variables": {
"C": "set of crates",
"R_c": "per-crate reports root",
"G_c": "graph artifacts for crate c",
"A_c": "analysis outputs for crate c",
"I_c": "invariant outputs for crate c",
"W": "workspace-level outputs",
"E": "kernel event stream",
"T": "event runtime",
"P": "tlog replay"
},
"equations": [
"R = union(R_c) + W",
"G_c = replay(E_c)",
"A_c = analysis(G_c)",
"I_c = invariants(G_c, A_c)",
"W = workspace(union(G_c), union(A_c), union(I_c))",
"good = max(intelligence, efficiency, correctness, alignment, robustness, performance, scalability, determinism, transparency, collaboration, empowerment, benefit, learning, future_proofing)"
],
"explanation": "Yes: make reports_out per crate, with a separate workspace layer. event-runtime should route each crate/session into its own output root. canon-analysis should stop assuming one flat output dir and operate on crate roots plus optional workspace aggregation.",
"agent_plan": {
"goal": "Refactor reports output layout from flat/shared to per-crate + workspace aggregation without breaking replay, analysis, or invariants.",
"phases": [
{
"name": "Phase 1: Define canonical layout",
"actions": [
"Adopt canonical root: state/reports_out/crates/<crate_name>/",
"Under each crate root create: graph/, graphs/, analysis/, metrics/, invariants/, meta/",
"Adopt workspace root: state/reports_out/workspace/",
"Move cross-crate/global outputs only into workspace/"
],
"target_layout": {
"crate_root": [
"graph/nodes.csv",
"graph/edges.csv",
"graph/files.txt",
"graph/graph.bin",
"graph/graph_snapshot.bin",
"graph/node_features.bin",
"graph/symbols.json",
"graph/symbol_spans.jsonl",
"graph/snapshot.meta.json",
"graphs/cfg.csv",
"graphs/callgraph.csv",
"graphs/modulegraph.csv",
"graphs/typegraph.csv",
"analysis/cycles.json",
"analysis/dead_code.json",
"analysis/diagnostics.json",
"analysis/hotspots.json",
"analysis/semantic_clusters.json",
"analysis/semantic_outliers.json",
"analysis/callsite_resolution.json",
"metrics/branch_complexity_report.json",
"metrics/branch_pressure_report.json",
"metrics/callgraph_centrality_report.json",
"metrics/dataflow_fanout_report.json",
"metrics/structural_hotspots_report.json",
"metrics/merge_candidates_report.json",
"metrics/path_redundancy_report.json",
"metrics/reachability_report.json",
"metrics/graph_health.json",
"metrics/system_health.json",
"metrics/tlog_integrity.json",
"metrics/upg_invariants.json",
"metrics/semantic_signatures.csv",
"metrics/cluster_graph.bin",
"invariants/invariant_candidates.json",
"invariants/invariants_discovered.json",
"invariants/invariant_validated.json",
"invariants/invariant_report.json",
"invariants/violations.json",
"meta/history.json"
],
"workspace_root": [
"global_callgraph.csv",
"global_dependency_cycles.json",
"global_invariant_report.json",
"global_violations.json",
"history.json"
]
}
},
{
"name": "Phase 2: Introduce path model",
"actions": [
"Create a shared path builder type, e.g. ReportLayout or OutputLayout",
"Input: root_dir + crate_name",
"Methods: crate_graph_dir(), crate_graphs_dir(), crate_analysis_dir(), crate_metrics_dir(), crate_invariants_dir(), crate_meta_dir(), workspace_dir()",
"Remove ad-hoc string concatenation across canon-analysis, event-runtime, and tlog-replay"
],
"targets": [
"canon-analysis/src/bin/analysis_engine.rs",
"canon-analysis/src/smt/reports.rs",
"canon-analysis/src/invariants/invariant_validator.rs",
"canon-analysis/src/invariants/kernel_invariants.rs",
"canon-analysis/src/repair/error_surface.rs",
"canon-analysis/src/smt/augment.rs",
"event-runtime/src/lib.rs",
"tlog-replay/src/replay.rs",
"tlog-replay/src/snapshot.rs"
]
},
{
"name": "Phase 3: Separate graph outputs from report outputs",
"actions": [
"Stop writing all files into one flat out_dir",
"Write structural graph artifacts only into graph/ and graphs/",
"Write algorithmic report JSON only into analysis/ and metrics/",
"Write invariant pipeline outputs only into invariants/ and meta/"
],
"specific_mapping": {
"graph": [
"nodes.csv",
"edges.csv",
"files.txt",
"graph.bin",
"graph_snapshot.bin",
"node_features.bin",
"symbols.json",
"symbol_spans.jsonl",
"snapshot.meta.json"
],
"graphs": [
"cfg.csv",
"callgraph.csv",
"modulegraph.csv",
"typegraph.csv"
],
"analysis": [
"cycles.json",
"dead_code.json",
"diagnostics.json",
"hotspots.json",
"semantic_clusters.json",
"semantic_outliers.json",
"callsite_resolution.json"
],
"metrics": [
"branch_complexity_report.json",
"branch_pressure_report.json",
"callgraph_centrality_report.json",
"dataflow_fanout_report.json",
"structural_hotspots_report.json",
"merge_candidates_report.json",
"path_redundancy_report.json",
"reachability_report.json",
"graph_health.json",
"system_health.json",
"tlog_integrity.json",
"upg_invariants.json",
"semantic_signatures.csv",
"cluster_graph.bin"
],
"invariants": [
"invariant_candidates.json",
"invariants_discovered.json",
"invariant_validated.json",
"invariant_report.json",
"violations.json"
],
"meta": [
"history.json"
]
}
},
{
"name": "Phase 4: Make event-runtime crate-aware",
"actions": [
"Extend runtime processing so each session resolves a crate identity",
"Use crate identity to select output root state/reports_out/crates/<crate_name>/",
"Maintain per-crate state if needed, or at minimum per-crate output partitioning",
"Do not merge unrelated crate sessions into one kernel directory"
],
"notes": [
"If crate name is present in session metadata, use it",
"Otherwise derive from project/module root deterministically",
"Fallback must be explicit, e.g. crates/unknown/"
]
},
{
"name": "Phase 5: Make analysis_engine operate on layout roots",
"actions": [
"Change CLI contract from generic dir/out_dir to crate_root or graph_dir + reports_root derived from layout",
"Support --crate-name for direct routing",
"Support --workspace aggregation mode separately",
"Ensure dir_mode uses crate-scoped paths and never writes into shared flat directories"
],
"notes": [
"Keep backwards compatibility only if trivial",
"Prefer one canonical mode over many partial modes"
]
},
{
"name": "Phase 6: Refactor invariant pipeline boundaries",
"actions": [
"run_invariant_pipeline(graph_dir) should write only into crate invariants/meta dirs",
"Keep discovered/validated/violations/history crate-local by default",
"Add separate workspace invariant aggregation pass later",
"Do not mix crate-local invariant history with workspace-global history"
]
},
{
"name": "Phase 7: Add workspace aggregator",
"actions": [
"Create a dedicated workspace aggregation stage after crate analyses complete",
"Read per-crate graph/report outputs",
"Emit global dependency cycles, global callgraph, and workspace invariant summaries",
"Store all such outputs only under reports_out/workspace/"
],
"notes": [
"Do not force every existing analysis to become cross-crate immediately",
"Start with aggregation-friendly metrics only"
]
},
{
"name": "Phase 8: Preserve replay/snapshot determinism",
"actions": [
"Ensure replay_graph_from_tlog_incremental reads and writes snapshot/meta from crate graph dir",
"Snapshot metadata must be crate-local",
"Verify repeated replay of same crate yields identical graph outputs",
"Verify workspace aggregation is pure function of crate outputs"
]
},
{
"name": "Phase 9: Cleanup legacy flat layout",
"actions": [
"Stop writing to reports_out/kernel and reports_out/reports",
"Add one migration utility to move existing files into new layout where safe",
"Delete or ignore legacy directories after verification",
"Update scripts such as state/reports.sh to use new layout"
]
},
{
"name": "Phase 10: Verification matrix",
"actions": [
"Single crate replay -> outputs land only in crates/<name>/",
"Two crate replay -> outputs isolated correctly",
"Incremental replay -> only touched crate updates",
"Invariant pipeline -> crate-local outputs appear in invariants/meta",
"Workspace aggregator -> global outputs appear only in workspace/",
"No legacy flat files are rewritten"
],
"acceptance_checks": [
"rg -n "reports_out/kernel|reports_out/reports" . returns only migration/compat code or zero",
"cargo build across canon-utils succeeds",
"Representative tlog replay produces expected per-crate tree",
"Snapshot + replay equivalence tests still pass"
]
}
],
"recommended_execution_order": [
"1. add shared path/layout type",
"2. refactor tlog-replay snapshot/replay paths",
"3. refactor canon-analysis writers to category dirs",
"4. refactor event-runtime to route by crate",
"5. update analysis_engine CLI",
"6. add workspace aggregator",
"7. migrate scripts and verify"
],
"most_important_structural_rule": "crate-local production first, workspace aggregation second",
"anti_patterns_to_avoid": [
"one shared reports_out/kernel for all crates",
"mixing graph artifacts and analysis reports in same directory",
"writing workspace/global artifacts into crate directories",
"path construction duplicated across crates",
"implicit crate detection without deterministic fallback"
]
}
}

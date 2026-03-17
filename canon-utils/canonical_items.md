# Canonical Item Registry

One entry per concept. If you need something, look here first.
Format: `CrateName` → `path::to::Item` — source file

---

## Events

| Item                   | Canonical location                      | Source file                                 |
|------------------------+-----------------------------------------+---------------------------------------------|
| `RuntimeEvent`         | `canon_event::RuntimeEvent`             | `canon-event/src/events.rs`                 |
| `RuntimeConsumer`      | `canon_event::RuntimeConsumer`          | `canon-event/src/events.rs`                 |
| `RuntimeEmitter`       | `canon_event::RuntimeEmitter`           | `canon-event/src/events.rs`                 |
| `RuntimeEmitterHandle` | `canon_event::RuntimeEmitterHandle`     | `canon-event/src/events.rs`                 |
| `RuntimeEventFilter`   | `canon_event::RuntimeEventFilter`       | `canon-event/src/events.rs`                 |
| `CapabilityRequested`  | `canon_event::CapabilityRequested`      | `canon-event/src/events.rs`                 |
| `CapabilityCompleted`  | `canon_event::CapabilityCompleted`      | `canon-event/src/events.rs`                 |
| `CapabilityFailed`     | `canon_event::CapabilityFailed`         | `canon-event/src/events.rs`                 |
| `NodeReady`            | `canon_event::NodeReady`                | `canon-event/src/events.rs`                 |
| `NodeStarted`          | `canon_event::NodeStarted`              | `canon-event/src/events.rs`                 |
| `NodeCompleted`        | `canon_event::NodeCompleted`            | `canon-event/src/events.rs`                 |
| `NodeFailed`           | `canon_event::NodeFailed`               | `canon-event/src/events.rs`                 |
| `EditEvent`            | `canon_event::EditEvent`                | `canon-event/src/events.rs`                 |
| `RustcEventConsumer`   | `canon_event::RustcEventConsumer`       | `canon-event/src/events.rs`                 |
| `impl_rustc_consumer!` | `canon_event::impl_rustc_consumer`      | `canon-event/src/lib.rs` (macro)            |
| `RustcEvent`           | `canon_event::RustcEvent`               | `canon-types/src/kernel_types_generated.rs` |
| `RustcState`           | `canon_event::RustcState`               | `canon-types/src/kernel_types_generated.rs` |
| `EventDelta`           | `canon_event::EventDelta`               | `canon-types/src/kernel_types_generated.rs` |
| `SupervisorEvent`      | `canon_event_store::SupervisorEvent`    | `canon-event-store/src/reader.rs`           |
| `CanonEvent`           | `canon_event::CanonEvent`               | `canon-event/src/tlog/event.rs`             |

---

## Tlog — writing

| Item                     | Canonical location                                             | Source file                               |
|--------------------------+----------------------------------------------------------------+-------------------------------------------|
| `canon_emit!`            | `canon_event::canon_emit`                                      | `canon-event/src/macros/emit.rs` ✅ **USE THIS** |
| `write_event_auto`       | `canon_event::write_event_auto`                                | `canon-event/src/emit.rs` *(when TlogEvent already constructed)* |
| `resolve_tlog_path`      | `canon_event::resolve_tlog_path`                               | `canon-event/src/emit.rs`                 |
| `canon_event_struct!`    | `canon_event::canon_event_struct`                              | `canon-event/src/macros/event.rs`         |
| `canon_event_enum!`      | `canon_event::canon_event_enum`                                | `canon-event/src/macros/event.rs`         |
| `BinarySegmentWriter`    | `canon_event::BinarySegmentWriter`                             | `canon-event/src/tlog/binary.rs` *(internal / batch only)* |
| `TlogWriter`             | `canon_event::TlogWriter`                                      | `canon-event/src/tlog/writer.rs` *(internal)* |
| `emit_event_json`        | `canon_event::emit_event_json`                                 | `canon-event/src/tlog/writer.rs` *(internal)* |
| `emit_event`             | `canon_event::emit_event`                                      | `canon-event/src/emit.rs` *(use `canon_emit!` instead)* |

---

## Tlog — reading

| Item                                       | Canonical location                                            | Source file                       |
|--------------------------------------------+---------------------------------------------------------------+-----------------------------------|
| `AnyEvent`                                 | `canon_event_store::AnyEvent`                                 | `canon-event-store/src/reader.rs` |
| `read_any_events_from_path`                | `canon_event_store::read_any_events_from_path`                | `canon-event-store/src/reader.rs` |
| `read_any_events_from_path_with_start_seq` | `canon_event_store::read_any_events_from_path_with_start_seq` | `canon-event-store/src/reader.rs` |
| `extract_rustc_event`                      | `canon_event_store::extract_rustc_event`                      | `canon-event-store/src/reader.rs` |
| `extract_capability_request`               | `canon_event_store::extract_capability_request`               | `canon-event-store/src/reader.rs` |
| `extract_edit_event`                       | `canon_event_store::extract_edit_event`                       | `canon-event-store/src/reader.rs` |
| `extract_supervisor_event`                 | `canon_event_store::extract_supervisor_event`                 | `canon-event-store/src/reader.rs` |
| `detect_tlog_format`                       | `canon_event_store::detect_tlog_format`                       | `canon-event-store/src/reader.rs` |

---

## Tlog — snapshots

| Item                       | Canonical location                            | Source file                             |
|----------------------------+-----------------------------------------------+-----------------------------------------|
| `save_graph_snapshot`      | `canon_event_store::save_graph_snapshot`      | `canon-event-store/src/snapshot.rs`     |
| `load_graph_snapshot`      | `canon_event_store::load_graph_snapshot`      | `canon-event-store/src/snapshot.rs`     |
| `SnapshotMeta`             | `canon_event_store::SnapshotMeta`             | `canon-event-store/src/snapshot.rs`     |
| `CodeSnapshot`             | `canon_event_store::CodeSnapshot`             | `canon-event-store/src/snapshot.rs`     |
| `CodeSnapshotNode`         | `canon_event_store::CodeSnapshotNode`         | `canon-event-store/src/snapshot.rs`     |
| `CodeSnapshotEdge`         | `canon_event_store::CodeSnapshotEdge`         | `canon-event-store/src/snapshot.rs`     |
| `find_last_session_offset` | `canon_event_store::find_last_session_offset` | `canon-event-store/src/session_scan.rs` |

---

## Graph — code graph (Rust AST)

| Item                                 | Canonical location                                      | Source file                            |
|--------------------------------------+---------------------------------------------------------+----------------------------------------|
| `CodeNode`                           | `canon_event_store::CodeNode`                           | `canon-event-store/src/graph_types.rs` |
| `CodeEdge`                           | `canon_event_store::CodeEdge`                           | `canon-event-store/src/graph_types.rs` |
| `CodeGraphState`                     | `canon_event_store::CodeGraphState`                     | `canon-event-store/src/graph_types.rs` |
| `apply_rustc_event_to_graph`         | `canon_event_store::apply_rustc_event_to_graph`         | `canon-event-store/src/replay.rs`      |
| `replay_graph_from_tlog`             | `canon_event_store::replay_graph_from_tlog`             | `canon-event-store/src/replay.rs`      |
| `replay_graph_from_tlog_incremental` | `canon_event_store::replay_graph_from_tlog_incremental` | `canon-event-store/src/replay.rs`      |
| `CodeGraph` *(materialised)*         | `canon_graph::CodeGraph`                                | `canon-graph/src/artifacts_loader.rs`  |
| `CsrGraph`                           | `canon_graph::CsrGraph`                                 | `canon-graph/src/artifacts_loader.rs`  |
| `load_code_graph`                    | `canon_graph::load_code_graph`                          | `canon-graph/src/artifacts_loader.rs`  |
| `GraphConsumer`                      | `canon_graph::GraphConsumer`                            | `canon-graph/src/consumer.rs`          |
| `GraphEdge` *(alias)*                | `canon_graph::GraphEdge` = `canon_event_store::CodeEdge` | `canon-graph/src/artifacts_loader.rs`  |
| `GraphNode`                          | `canon_graph::GraphNode`                                | `canon-graph/src/artifacts_loader.rs`  |

---

## Graph — goal graph (task planning DAG)

Types live in `canon-goal` (Cluster A); runtime/LLM code stays in `canon-agent` (Cluster B).
Both `canon_goal::X` and `canon_agent::X` resolve to the same type (`canon-agent` re-exports `canon-goal`).

| Item                                   | Canonical location                              | Source file                         |
|----------------------------------------+-------------------------------------------------+-------------------------------------|
| `GoalGraph`                            | `canon_goal::goal_graph::GoalGraph`             | `canon-goal/src/goal_graph.rs`      |
| `GoalNode`                             | `canon_goal::goal_graph::GoalNode`              | `canon-goal/src/goal_graph.rs`      |
| `NodeStatus`                           | `canon_goal::goal_graph::NodeStatus`            | `canon-goal/src/goal_graph.rs`      |
| `task_graph_resolve_ready`             | `canon_goal::goal_graph::task_graph_resolve_ready` | `canon-goal/src/goal_graph.rs`   |
| `GoalGraphPatch`                       | `canon_goal::goal_patch::GoalGraphPatch`        | `canon-goal/src/goal_patch.rs`      |
| `GoalGraphEvent`                       | `canon_goal::goal_patch::GoalGraphEvent`        | `canon-goal/src/goal_patch.rs`      |
| `apply_graph_patch`                    | `canon_goal::goal_patch::apply_graph_patch`     | `canon-goal/src/goal_patch.rs`      |
| `DecomposeTaskSpec`                    | `canon_goal::decompose::DecomposeTaskSpec`      | `canon-goal/src/decompose.rs`       |
| `DecomposeNodeType`                    | `canon_goal::decompose::DecomposeNodeType`      | `canon-goal/src/decompose.rs`       |
| `graph_analysis_compute_graph_signals` | `canon_agent::graph_algo::graph_analysis_compute_graph_signals` | `canon-agent/src/graph_algo.rs` |

---

## Graph — goal graph projector (event-derived)

| Item                            | Canonical location                                 | Source file                                     |
|---------------------------------+----------------------------------------------------+-------------------------------------------------|
| `GoalGraphState`                | `canon_event_store::GoalGraphState`                | `canon-event-store/src/goal_graph_projector.rs` |
| `GoalNodeState`                 | `canon_event_store::GoalNodeState`                 | `canon-event-store/src/goal_graph_projector.rs` |
| `replay_goal_graph_from_tlog`   | `canon_event_store::replay_goal_graph_from_tlog`   | `canon-event-store/src/goal_graph_projector.rs` |
| `replay_goal_graph_incremental` | `canon_event_store::replay_goal_graph_incremental` | `canon-event-store/src/goal_graph_projector.rs` |

---

## Graph — capability graph projector (event-derived)

| Item                                | Canonical location                                     | Source file                                           |
|-------------------------------------+--------------------------------------------------------+-------------------------------------------------------|
| `CapabilityGraphState`              | `canon_event_store::CapabilityGraphState`              | `canon-event-store/src/capability_graph_projector.rs` |
| `CapabilityOpNode`                  | `canon_event_store::CapabilityOpNode`                  | `canon-event-store/src/capability_graph_projector.rs` |
| `CapabilityOpEdge`                  | `canon_event_store::CapabilityOpEdge`                  | `canon-event-store/src/capability_graph_projector.rs` |
| `replay_capability_graph_from_tlog` | `canon_event_store::replay_capability_graph_from_tlog` | `canon-event-store/src/capability_graph_projector.rs` |
| *(wires `tool_result` events)*      | `CapabilityOpNode::result: Option<Value>`              | populated from `tool_result` tlog events              |

---

## Types — code graph primitives

| Item              | Canonical location             | Source file                        |
|-------------------+--------------------------------+------------------------------------|
| `NodeKind`        | `canon_types::NodeKind`        | `canon-types/src/types.rs`         |
| `EdgeKind`        | `canon_types::EdgeKind`        | `canon-types/src/types.rs`         |
| `Node` *(typed)*  | `canon_types::Node`            | `canon-types/src/types.rs`         |
| `Edge` *(typed)*  | `canon_types::Edge`            | `canon-types/src/types.rs`         |
| `Metadata`        | `canon_types::Metadata`        | `canon-types/src/types.rs`         |
| `SpanRange`       | `canon_types::SpanRange`       | `canon-types/src/types.rs`         |
| `SCHEMA_VERSION`  | `canon_types::SCHEMA_VERSION`  | `canon-types/src/types.rs`         |
| `parse_node_kind` | `canon_types::parse_node_kind` | `canon-types/src/types.rs`         |
| `parse_edge_kind` | `canon_types::parse_edge_kind` | `canon-types/src/types.rs`         |
| `ReportLayout`    | `canon_types::ReportLayout`    | `canon-types/src/report_layout.rs` |

---

## Planning — goals and capabilities

| Item                 | Canonical location                              | Source file                         |
|----------------------+-------------------------------------------------+-------------------------------------|
| `GoalSpec`           | `canon_goal::goal::GoalSpec`                    | `canon-goal/src/goal.rs`            |
| `GoalType`           | `canon_goal::goal::GoalType`                    | `canon-goal/src/goal.rs`            |
| `PipelineCapability` | `canon_goal::capability_types::PipelineCapability` | `canon-goal/src/capability_types.rs` |
| `CapabilityMode`     | `canon_goal::capability_types::CapabilityMode`  | `canon-goal/src/capability_types.rs` |
| `CapabilityConfig`   | `canon_agent::config::CapabilityConfig`         | `canon-agent/src/config.rs`         |

---

## Planning — LLM dispatch

| Item                                                   | Canonical location                                                         | Source file                                  |
|--------------------------------------------------------+----------------------------------------------------------------------------+----------------------------------------------|
| `llm_worker_new_tabs`                                  | `canon_agent::endpoint_worker::llm_worker_new_tabs`                        | `canon-agent/src/endpoint_worker.rs`         |
| `llm_client_call_agent_raw_with_retry_allow_mismatch`  | `canon_agent::llm::llm_client_call_agent_raw_with_retry_allow_mismatch`    | `canon-agent/src/llm.rs`                     |
| `llm_client_call_agent_json_with_retry_allow_mismatch` | `canon_agent::llm::llm_client_call_agent_json_with_retry_allow_mismatch`   | `canon-agent/src/llm.rs`                     |
| `llm_worker_init_workers`                              | `canon_agent::endpoint_worker::llm_worker_init_workers`                    | `canon-agent/src/endpoint_worker.rs`         |
| `LlmExecutorConsumer`                                  | `canon_kernel::consumers::llm_executor::LlmExecutorConsumer`               | `canon-kernel/src/consumers/llm_executor.rs` |

---

## Runtime — kernel and consumers

| Item                            | Canonical location                              | Source file                                         |
|---------------------------------+-------------------------------------------------+-----------------------------------------------------|
| `EventRuntime`                  | `canon_kernel::EventRuntime`                    | `canon-kernel/src/lib.rs`                           |
| `register_default_capabilities` | `canon_kernel::register_default_capabilities`   | `canon-kernel/src/lib.rs`                           |
| `PromptRegistry`                | `canon_kernel::bootstrap::PromptRegistry`       | `canon-kernel/src/bootstrap.rs`                     |
| `PromptRegistryHandle`          | `canon_kernel::bootstrap::PromptRegistryHandle` | `canon-kernel/src/bootstrap.rs`                     |
| `bootstrap_config`              | `canon_kernel::bootstrap::bootstrap_config`     | `canon-kernel/src/bootstrap.rs`                     |
| `AgentConsumer` *(private)*     | `canon_kernel::consumers::agent::AgentConsumer` | `canon-kernel/src/consumers/agent/mod.rs`           |
| `LlmExecutorConsumer`           | `canon_kernel::consumers::llm_executor`         | `canon-kernel/src/consumers/llm_executor.rs`        |
| `EventLoopConsumer`             | `canon_kernel::consumers::event_loop`           | `canon-kernel/src/consumers/event_loop.rs`          |
| `CapabilityExecutorConsumer`    | `canon_kernel::consumers::capability_executor`  | `canon-kernel/src/consumers/capability_executor.rs` |

---

## Capabilities — registry and execution

| Item                             | Canonical location                               | Source file                          |
|----------------------------------+--------------------------------------------------+--------------------------------------|
| `CapabilityRegistry`             | `canon_capability::CapabilityRegistry`           | `canon-capability/src/registry.rs`   |
| `CapabilityContext`              | `canon_capability::CapabilityContext`            | `canon-capability/src/context.rs`    |
| `CapabilityResult`               | `canon_capability::CapabilityResult`             | `canon-capability/src/result.rs`     |
| `Capability` *(trait)*           | `canon_capability::Capability`                   | `canon-capability/src/trait.rs`      |
| `register_editor_capabilities`   | `canon_editor::register_editor_capabilities`     | `canon-editor/src/capabilities.rs`   |
| `register_analysis_capabilities` | `canon_analysis::register_analysis_capabilities` | `canon-analysis/src/capabilities.rs` |
| `register_build_capabilities`    | `canon_supervisor::register_build_capabilities`  | `canon-supervisor/src/executor.rs`   |

---

## Analysis — code graph analysis

| Item                         | Canonical location                           | Source file                                            |
|------------------------------+----------------------------------------------+--------------------------------------------------------|
| `generate_reports`           | `canon_analysis::generate_reports`           | `canon-analysis/src/report_pipeline.rs`                |
| `generate_reports_from_tlog` | `canon_analysis::generate_reports_from_tlog` | `canon-analysis/src/report_pipeline.rs`                |
| `run_invariant_pipeline`     | `canon_analysis::run_invariant_pipeline`     | `canon-analysis/src/invariants/invariant_validator.rs` |
| `ReportEventConsumer`        | `canon_analysis::ReportEventConsumer`        | `canon-analysis/src/report_consumer.rs`                |
| `CapabilityEventConsumer`    | `canon_analysis::CapabilityEventConsumer`    | `canon-analysis/src/capability_consumer.rs`            |
| `SmtConsumer`                | `canon_analysis::SmtConsumer`                | `canon-analysis/src/smt/consumer.rs`                   |

---

## Logging / debug output

| Item    | Canonical location               | Source file                     |
|---------+----------------------------------+---------------------------------|
| `info`  | `canon_event::emit_debug::info`  | `canon-event/src/emit_debug.rs` |
| `warn`  | `canon_event::emit_debug::warn`  | `canon-event/src/emit_debug.rs` |
| `error` | `canon_event::emit_debug::error` | `canon-event/src/emit_debug.rs` |

---

## Editor / mutations

| Item                  | Canonical location                  | Source file                        |
|-----------------------+-------------------------------------+------------------------------------|
| `SymbolIndex`         | `canon_editor::SymbolIndex`         | `canon-editor/src/symbol_index.rs` |
| `EditOp`              | `canon_editor::EditOp`              | `canon-editor/src/structured.rs`   |
| `rename_symbol_pairs` | `canon_editor::rename_symbol_pairs` | `canon-editor/src/lib.rs`          |
| `EditConsumer`        | `canon_editor::EditConsumer`        | `canon-editor/src/consumer.rs`     |

---

## Supervisor / build

| Item               | Canonical location                   | Source file                       |
|--------------------+--------------------------------------+-----------------------------------|
| `SupervisorConfig` | `canon_supervisor::SupervisorConfig` | `canon-supervisor/src/config.rs`  |
| `ProcessManager`   | `canon_supervisor::ProcessManager`   | `canon-supervisor/src/process.rs` |
| `start_watcher`    | `canon_supervisor::start_watcher`    | `canon-supervisor/src/watcher.rs` |
| `affected_crates`  | `canon_supervisor::affected_crates`  | `canon-supervisor/src/watcher.rs` |

---

## Deprecated / removed

| Item                                               | Status     | Replacement                                                            |
|----------------------------------------------------+------------+------------------------------------------------------------------------|
| `canon_planner::*`                                 | ✅ Deleted | Import directly from `canon_agent`, `canon_goal`, `canon_graph`, `canon_analysis` |
| `canon_agent_v3` (crate)                           | ✅ Renamed | `canon_agent` (package name + directory)                               |
| `canon_agent_v3::engine`                           | ✅ Deleted | `canon_agent::llm` / `canon_agent::endpoint_worker`                    |
| `canon_agent_v3::state_snapshot::PipelineSnapshot` | ✅ Deleted | `GoalGraphCheckpointed` + event log projection                         |
| `RuntimeEvent::AgentState`                         | ✅ Removed | `RuntimeEvent::GoalGraphCheckpointed`                                  |
| `canon_graph::Edge` *(duplicate struct)*           | ✅ Unified | `canon_graph::GraphEdge` = `canon_event_store::CodeEdge`               |
| `rebuild_symbol_index` in `canon_graph`            | ✅ Deleted | `canon_event_store::replay::rebuild_symbol_index`                      |
| `append_event` / `append_event_json`               | ✅ Renamed | `write_event` (method) / `emit_event_json` (free fn)                   |
| `canon_event_store::writer` shim module            | ✅ Deleted | Import `BinarySegmentWriter`, `TlogWriter`, `CanonEvent`, `emit_event_json` directly from `canon_event::` |
| `emit_rustc_event`                                 | ✅ Deleted | `canon_emit!("canon-rustc", kind, payload, path)`                      |
| `emit_capability_event`                            | ✅ Deleted | `canon_emit!("canon-runtime", kind, payload, path)`                    |
| `emit_edit_event`                                  | ✅ Deleted | `canon_emit!("canon-editor", "edit_event", payload, &resolve_tlog_path(Some(root), None))` |
| `BinaryTlogWriter`                                 | ✅ Superseded | `canon_emit!` / `write_event_auto` handle format selection            |
| `KernelGraph` / `KernelCodeGraph`                  | ✅ Renamed | `CodeGraph` (materialised) / `CodeGraphState` (event-sourced)          |
| `NodeRow` / `EdgeRow` *(graph_types.rs)*           | ✅ Renamed | `CodeNode` / `CodeEdge`                                                |
| `KernelSnapshot*`                                  | ✅ Renamed | `CodeSnapshot` / `CodeSnapshotNode` / `CodeSnapshotEdge`               |
| `ProjectedGoalNode`                                | ✅ Renamed | `GoalNodeState`                                                        |
| `dag.rs` / `planner_update.rs`                     | ✅ Renamed | `goal_graph.rs` / `goal_patch.rs`                                      |
| `canon_agent::goal_*` / `canon_agent::capability_types` / `canon_agent::decompose` | → moved | `canon_goal::*` (re-exported from `canon_agent`) |

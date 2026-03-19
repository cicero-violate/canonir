# Canonical Item Registry

One entry per concept. If you need something, look here first.
Format: `CrateName` → `path::to::Item` — source file

---

## Events

| Item                   | Canonical location                   | Source file                                      |
|------------------------+--------------------------------------+--------------------------------------------------|
| `CanonEvent`           | `canon_event::CanonEvent`            | `canon-runtime-events/src/events.rs`             |
| `EventConsumer`        | `canon_event::EventConsumer`         | `canon-runtime-events/src/events.rs`             |
| `EventEmitter`         | `canon_event::EventEmitter`          | `canon-runtime-events/src/events.rs`             |
| `EventEmitterHandle`   | `canon_event::EventEmitterHandle`    | `canon-runtime-events/src/events.rs`             |
| `EventFilter`          | `canon_event::EventFilter`           | `canon-runtime-events/src/events.rs`             |
| `EventMask`            | `canon_event::EventMask`             | `canon-runtime-events/src/events.rs`             |
| `ErrorOccurred`        | `canon_event::ErrorOccurred`         | `canon-runtime-events/src/events.rs`             |
| `CapabilityRequested`  | `canon_event::CapabilityRequested`   | `canon-runtime-events/src/events.rs`             |
| `CapabilityCompleted`  | `canon_event::CapabilityCompleted`   | `canon-runtime-events/src/events.rs`             |
| `CapabilityFailed`     | `canon_event::CapabilityFailed`      | `canon-runtime-events/src/events.rs`             |
| `NodeReady`            | `canon_event::NodeReady`             | `canon-runtime-events/src/events.rs`             |
| `NodeStarted`          | `canon_event::NodeStarted`           | `canon-runtime-events/src/events.rs`             |
| `NodeCompleted`        | `canon_event::NodeCompleted`         | `canon-runtime-events/src/events.rs`             |
| `NodeFailed`           | `canon_event::NodeFailed`            | `canon-runtime-events/src/events.rs`             |
| `EditEvent`            | `canon_event::EditEvent`             | `canon-runtime-events/src/events.rs`             |
| `RustcEventConsumer`   | `canon_event::RustcEventConsumer`    | `canon-runtime-events/src/events.rs`             |
| `impl_rustc_consumer!` | `canon_event::impl_rustc_consumer`   | `canon-runtime-events/src/lib.rs` (macro)         |
| `RustcEvent`           | `canon_event::RustcEvent`            | `canon-types/src/kernel_types.rs`                |
| `RustcState`           | `canon_event::RustcState`            | `canon-types/src/kernel_types.rs`                |
| `EventDelta`           | `canon_event::EventDelta`            | `canon-types/src/kernel_types.rs`                |
| `SupervisorEvent`      | `canon_event_store::SupervisorEvent` | `canon-storage-eventlog/src/reader.rs`           |

---

## Tlog — writing

| Item                  | Canonical location                 | Source file                                                               |
|-----------------------+------------------------------------+---------------------------------------------------------------------------|
| `canon_emit!`         | `canon_event::canon_emit`          | `canon-runtime-events/src/macros/emit.rs` ✅ **USE THIS**                 |
| `write_event_auto`    | `canon_event::write_event_auto`    | `canon-runtime-events/src/emit.rs` *(when TlogEvent already constructed)* |
| `resolve_tlog_path`   | `canon_event::resolve_tlog_path`   | `canon-runtime-events/src/emit.rs`                                        |
| `canon_event_struct!` | `canon_event::canon_event_struct`  | `canon-runtime-events/src/macros/event.rs`                                |
| `canon_event_enum!`   | `canon_event::canon_event_enum`    | `canon-runtime-events/src/macros/event.rs`                                |
| `BinarySegmentWriter` | `canon_event::BinarySegmentWriter` | `canon-runtime-events/src/tlog/binary.rs` *(internal / batch only)*       |
| `TlogWriter`          | `canon_event::TlogWriter`          | `canon-runtime-events/src/tlog/writer.rs` *(internal)*                    |
| `emit_event_json`     | `canon_event::emit_event_json`     | `canon-runtime-events/src/tlog/writer.rs` *(internal)*                    |
| `emit_event`          | `canon_event::emit_event`          | `canon-runtime-events/src/emit.rs` *(use `canon_emit!` instead)*          |

---

## Tlog — reading

| Item                                       | Canonical location                                            | Source file                            |
|--------------------------------------------+---------------------------------------------------------------+----------------------------------------|
| `AnyEvent`                                 | `canon_event_store::AnyEvent`                                 | `canon-storage-eventlog/src/reader.rs` |
| `read_any_events_from_path`                | `canon_event_store::read_any_events_from_path`                | `canon-storage-eventlog/src/reader.rs` |
| `read_any_events_from_path_with_start_seq` | `canon_event_store::read_any_events_from_path_with_start_seq` | `canon-storage-eventlog/src/reader.rs` |
| `extract_rustc_event`                      | `canon_event_store::extract_rustc_event`                      | `canon-storage-eventlog/src/reader.rs` |
| `extract_capability_request`               | `canon_event_store::extract_capability_request`               | `canon-storage-eventlog/src/reader.rs` |
| `extract_edit_event`                       | `canon_event_store::extract_edit_event`                       | `canon-storage-eventlog/src/reader.rs` |
| `extract_supervisor_event`                 | `canon_event_store::extract_supervisor_event`                 | `canon-storage-eventlog/src/reader.rs` |
| `detect_tlog_format`                       | `canon_event_store::detect_tlog_format`                       | `canon-storage-eventlog/src/reader.rs` |

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

| Item                                 | Canonical location                                       | Source file                            |
|--------------------------------------+----------------------------------------------------------+----------------------------------------|
| `CodeNode`                           | `canon_event_store::CodeNode`                            | `canon-event-store/src/graph_types.rs` |
| `CodeEdge`                           | `canon_event_store::CodeEdge`                            | `canon-event-store/src/graph_types.rs` |
| `CodeGraphState`                     | `canon_event_store::CodeGraphState`                      | `canon-event-store/src/graph_types.rs` |
| `apply_rustc_event_to_graph`         | `canon_event_store::apply_rustc_event_to_graph`          | `canon-event-store/src/replay.rs`      |
| `replay_graph_from_tlog`             | `canon_event_store::replay_graph_from_tlog`              | `canon-event-store/src/replay.rs`      |
| `replay_graph_from_tlog_incremental` | `canon_event_store::replay_graph_from_tlog_incremental`  | `canon-event-store/src/replay.rs`      |
| `CodeGraph` *(materialised)*         | `canon_graph::CodeGraph`                                 | `canon-graph/src/artifacts_loader.rs`  |
| `CsrGraph`                           | `canon_graph::CsrGraph`                                  | `canon-graph/src/artifacts_loader.rs`  |
| `load_code_graph`                    | `canon_graph::load_code_graph`                           | `canon-graph/src/artifacts_loader.rs`  |
| `GraphConsumer`                      | `canon_graph::GraphConsumer`                             | `canon-graph/src/consumer.rs`          |
| `GraphEdge` *(alias)*                | `canon_graph::GraphEdge` = `canon_event_store::CodeEdge` | `canon-graph/src/artifacts_loader.rs`  |
| `GraphNode`                          | `canon_graph::GraphNode`                                 | `canon-graph/src/artifacts_loader.rs`  |

---

## Graph — goal graph (task planning DAG)

Types live in `canon-goal` (Cluster A); runtime/LLM code stays in `canon-agent` (Cluster B).
Both `canon_goal::X` and `canon_agent::X` resolve to the same type (`canon-agent` re-exports `canon-goal`).

| Item                                   | Canonical location                                              | Source file                     |
|----------------------------------------+-----------------------------------------------------------------+---------------------------------|
| `GoalGraph`                            | `canon_goal::goal_graph::GoalGraph`                             | `canon-goal/src/goal_graph.rs`  |
| `GoalNode`                             | `canon_goal::goal_graph::GoalNode`                              | `canon-goal/src/goal_graph.rs`  |
| `NodeStatus`                           | `canon_goal::goal_graph::NodeStatus`                            | `canon-goal/src/goal_graph.rs`  |
| `task_graph_resolve_ready`             | `canon_goal::goal_graph::task_graph_resolve_ready`              | `canon-goal/src/goal_graph.rs`  |
| `GoalGraphPatch`                       | `canon_goal::goal_patch::GoalGraphPatch`                        | `canon-goal/src/goal_patch.rs`  |
| `GoalGraphEvent`                       | `canon_goal::goal_patch::GoalGraphEvent`                        | `canon-goal/src/goal_patch.rs`  |
| `apply_graph_patch`                    | `canon_goal::goal_patch::apply_graph_patch`                     | `canon-goal/src/goal_patch.rs`  |
| `DecomposeTaskSpec`                    | `canon_goal::decompose::DecomposeTaskSpec`                      | `canon-goal/src/decompose.rs`   |
| `DecomposeNodeType`                    | `canon_goal::decompose::DecomposeNodeType`                      | `canon-goal/src/decompose.rs`   |
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

| Item                 | Canonical location                                 | Source file                          |
|----------------------+----------------------------------------------------+--------------------------------------|
| `GoalSpec`           | `canon_goal::goal::GoalSpec`                       | `canon-goal/src/goal.rs`             |
| `GoalType`           | `canon_goal::goal::GoalType`                       | `canon-goal/src/goal.rs`             |
| `PipelineCapability` | `canon_goal::capability_types::PipelineCapability` | `canon-goal/src/capability_types.rs` |
| `CapabilityMode`     | `canon_goal::capability_types::CapabilityMode`     | `canon-goal/src/capability_types.rs` |
| `CapabilityConfig`   | `canon_agent::config::CapabilityConfig`            | `canon-agent/src/config.rs`          |

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

| Item                            | Canonical location                                                  | Source file                                          |
|---------------------------------+---------------------------------------------------------------------+------------------------------------------------------|
| `EventRuntime`                  | `canon_runtime::EventRuntime`                                       | `canon-runtime/src/lib.rs`                           |
| `register_default_capabilities` | `canon_runtime::register_default_capabilities`                      | `canon-runtime/src/lib.rs`                           |
| `PromptRegistry`                | `canon_runtime::bootstrap::PromptRegistry`                          | `canon-runtime/src/bootstrap.rs`                     |
| `PromptRegistryHandle`          | `canon_runtime::bootstrap::PromptRegistryHandle`                    | `canon-runtime/src/bootstrap.rs`                     |
| `bootstrap_config`              | `canon_runtime::bootstrap::bootstrap_config`                        | `canon-runtime/src/bootstrap.rs`                     |
| `AgentConsumer`                 | `canon_runtime::consumers::agent::AgentConsumer`                    | `canon-runtime/src/consumers/agent/mod.rs`           |
| `LlmCapabilityHandler`          | `canon_runtime::consumers::llm_executor::LlmCapabilityHandler`      | `canon-runtime/src/consumers/llm_executor.rs`        |
| `CapabilityExecutor`            | `canon_runtime::consumers::capability_executor::CapabilityExecutor` | `canon-runtime/src/consumers/capability_executor.rs` |
| `ErrorLogger`                   | `canon_runtime::consumers::error_logger::ErrorLogger`               | `canon-runtime/src/consumers/error_logger.rs`        |
| `FailureStoreConsumer`          | `canon_runtime::consumers::failure_store::FailureStoreConsumer`     | `canon-runtime/src/consumers/failure_store.rs`       |

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
| `register_build_capabilities`    | `canon_builder::register_build_capabilities`     | `canon-builder/src/executor.rs`      |

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
| `RuntimeReachabilityReport`  | `canon_analysis::RuntimeReachabilityReport`  | `canon-analysis/src/report_types.rs`                   |

---

## Logging / debug output

| Item         | Canonical location                  | Source file                                      |
|--------------+-------------------------------------+--------------------------------------------------|
| `DebugEvent` | `canon_event::DebugEvent`           | `canon-runtime-events/src/events.rs`             |
| `canon_emit!` | `canon_event::canon_emit`          | `canon-runtime-events/src/macros/emit.rs`        |

---

## Errors — eventized + logs

| Item                       | Canonical location                                            | Source file                                       |
|----------------------------+---------------------------------------------------------------+---------------------------------------------------|
| `ErrorOccurred`            | `canon_event::ErrorOccurred`                                  | `canon-runtime-events/src/events.rs`              |
| `error_occurred` tlog kind | `CanonEvent::ErrorOccurred` → `TlogEvent`                     | `canon-runtime/src/lib.rs` (append_runtime_event) |
| Error log JSONL (default)  | `CANON_ERROR_LOG_PATH` → reports_out `error_log/errors.jsonl` | `canon-runtime/src/consumers/error_logger.rs`     |
| Failure stats JSON         | `CANON_FAILURE_STORE_PATH` → reports_out `failure_store.json` | `canon-runtime/src/consumers/failure_store.rs`    |

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

| Item               | Canonical location                | Source file                    |
|--------------------+-----------------------------------+--------------------------------|
| `SupervisorConfig` | `canon_builder::SupervisorConfig` | `canon-builder/src/config.rs`  |
| `ProcessManager`   | `canon_builder::ProcessManager`   | `canon-builder/src/process.rs` |
| `start_watcher`    | `canon_builder::start_watcher`    | `canon-builder/src/watcher.rs` |
| `affected_crates`  | `canon_builder::affected_crates`  | `canon-builder/src/watcher.rs` |

---

## Deprecated / removed

| Item                                                                               | Status        | Replacement                                                                                               |
|------------------------------------------------------------------------------------+---------------+-----------------------------------------------------------------------------------------------------------|
| `canon_agent_v3` (crate)                                                           | ✅ Renamed    | `canon_agent` (package name + directory)                                                                  |
| `RuntimeEvent::AgentState`                                                         | ✅ Removed    | `RuntimeEvent::GoalGraphCheckpointed`                                                                     |
| `canon_graph::Edge` *(duplicate struct)*                                           | ✅ Unified    | `canon_graph::GraphEdge` = `canon_event_store::CodeEdge`                                                  |
| `append_event` / `append_event_json`                                               | ✅ Renamed    | `write_event` (method) / `emit_event_json` (free fn)                                                      |
| `BinaryTlogWriter`                                                                 | ✅ Superseded | `canon_emit!` / `write_event_auto` handle format selection                                                |
| `KernelGraph` / `KernelCodeGraph`                                                  | ✅ Renamed    | `CodeGraph` (materialised) / `CodeGraphState` (event-sourced)                                             |
| `NodeRow` / `EdgeRow` *(graph_types.rs)*                                           | ✅ Renamed    | `CodeNode` / `CodeEdge`                                                                                   |
| `KernelSnapshot*`                                                                  | ✅ Renamed    | `CodeSnapshot` / `CodeSnapshotNode` / `CodeSnapshotEdge`                                                  |
| `ProjectedGoalNode`                                                                | ✅ Renamed    | `GoalNodeState`                                                                                           |
| `dag.rs` / `planner_update.rs`                                                     | ✅ Renamed    | `goal_graph.rs` / `goal_patch.rs`                                                                         |
| `canon_agent::goal_*` / `canon_agent::capability_types` / `canon_agent::decompose` | → moved       | `canon_goal::*` (re-exported from `canon_agent`)                                                          |

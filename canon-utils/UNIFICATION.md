# Unification Backlog

Goal: reduce LOC, eliminate duplication, tighten naming.
Each item is independently actionable. Ordered roughly by impact/effort ratio.

---

## 1. Dead RuntimeEvent variants — delete or wire up

`canon-event/src/events.rs` defines variants that `append_runtime_event` in
`canon-kernel/src/lib.rs` silently discards (`_ => return`):

- `NodeReady`, `NodeStarted`, `NodeCompleted`, `NodeFailed` — emitted, dispatched to bus, but never written to tlog
- `CapabilityRequested` — same
- `Tick`, `RuntimeStateUpdated` — intentionally skipped but not documented as such

Decision per variant: write to tlog, or delete the variant.
`NodeStarted`/`NodeCompleted`/`NodeFailed` should probably be written — they are the
primary signal the goal graph projector needs to track node status without relying on
`agent_state` snapshots.

---

## 2. `rebuild_symbol_index` — exact duplicate

Defined identically in:
- `canon-graph/src/graph/graph_builder.rs`
- `canon-event-store/src/replay.rs`

Delete one, import the other.

---

## 3. Node type proliferation — pick one canonical struct

Five types represent "a node in the code graph":

| Type                    | Crate                              | kind field                     |
|-------------------------+------------------------------------+--------------------------------|
| `Node`                  | `canon-types`                      | `NodeKind` (enum, 14 variants) |
| `Node`                  | `canon-graph/artifacts_loader.rs`  | `String`                       |
| `NodeRow`               | `canon-event-store/graph_types.rs` | `String`                       |
| `ModuleNode`            | `canon-graph/graph/graph_types.rs` | implied                        |
| `KernelCodeGraph.nodes` | `canon-event-store`                | mix                            |

Unify to `NodeRow` (event-store layer) + `canon-types::NodeKind` enum for the kind.
Delete the others. `ModuleNode` and the `canon-graph` `Node` are redundant wrappers.

---

## 4. `canon-planner` — pure facade, delete it

`canon-planner/src/lib.rs` is 100% re-exports from `canon-agent-v3`, `canon-graph`,
and `canon-analysis`. It adds no logic and creates an extra indirection layer.

Replace all `canon_planner::` imports with direct imports from the source crates.
Delete the crate.

---

## 5. `canon-agent-v3/src/engine.rs` — pure pass-through, delete it

Only re-exports two LLM call functions from the `llm` module with identical signatures.
Callers (`llm_executor.rs`) should import directly from `canon_agent_v3::llm`.
Delete `engine.rs`.

---

## 6. `canon-agent-v3/src/state_snapshot.rs` — superseded

`PipelineSnapshot` was the old snapshot mechanism. The new architecture uses
`GoalGraphCheckpointed { tlog_seq }` + the goal graph projector.

Check if `PipelineSnapshot` is still constructed anywhere. If not, delete the file.
`snapshot_store_save` / `snapshot_store_load` are filesystem-based and conflict with
the event-sourced approach.

---

## 7. CapabilityConfig struct names — strip redundant prefixes

In `canon-agent-v3/src/config.rs`:

| Current                            | Should be                                      |
|------------------------------------+------------------------------------------------|
| `CapabilityConfigRawRoleConfig`    | `RoleConfig`                                   |
| `CapabilityConfigLlmEndpoint`      | `LlmEndpoint`                                  |
| `CapabilityConfigGoalSpec`         | `GoalSpec` (or merge with `goal.rs::GoalSpec`) |
| `CapabilityConfigCapabilityPolicy` | `CapabilityPolicy`                             |

The outer type is already named `CapabilityConfig` — no need to prefix every inner type.

---

## 8. `GoalSpec` — defined twice

`canon-agent-v3/src/goal.rs` defines `GoalSpec`.
`canon-agent-v3/src/config.rs` defines `CapabilityConfigGoalSpec` covering the same concept.

Merge into one.

---

## 9. `EdgeKind` — enum vs string inconsistency (partial ✅)

`artifacts_loader::Edge` was a duplicate of `event-store::EdgeRow` (identical fields).
Replaced with `pub use canon_event_store::EdgeRow as Edge` — now one type.
`canon-graph/src/lib.rs` also exports `EdgeRow`/`NodeRow` as canonical names.

Remaining: `EdgeRow.kind` and `NodeRow.kind` are still `String`, while
`canon-types::Edge.kind` is `EdgeKind` (enum). Changing the storage layer to use the
enum requires handling unknown variants (no `Other(String)` variant exists in `EdgeKind`).
Decision deferred: keep `String` at the event/storage boundary as an intentional design choice.

---

## 10. RustcEventConsumer boilerplate — macro or blanket impl ✅

Added `impl_rustc_consumer!(Type, MASK, handler_fn)` macro to `canon-event/src/lib.rs`.
Applied to all 5 consumer files: each renames `on_event` body to `handle_event` and uses the macro.

---

## 11. `canon-graph` `#[allow(dead_code)]` structs — use or delete

`canon-graph/src/artifacts_loader.rs`:
```rust
#[allow(dead_code)]
pub struct CsrGraph { ... }
#[allow(dead_code)]
pub struct KernelGraph { ... }
```

If unused externally, delete. If needed, remove the allow and fix call sites.

---

## 12. Event write terminology — standardise to one verb ✅

`append_event_json` → `emit_event_json`, `TlogWriter::append_event` / `BinarySegmentWriter::append_event`
→ `write_event` throughout. All callers updated. Consistent "emit" verb at high level,
"write" at the struct-method level.

---

## 13. `canon-agent-v3` thin files — consolidate

Files under 100 LOC that wrap a single concept could be merged:
- `llm_provider.rs` (error type + one helper) → inline into `llm.rs`
- `gpu_scheduler.rs` (re-exports only) → inline into `gpu_scheduler_layout.rs`
- `capability.rs` (thin types) → merge with `capability_types.rs`

---

## 14. `canon-analysis` — consider splitting

Currently one crate hosts:
- SMT solving (`smt/` — ~1500 LOC)
- Query/JSONPath (`query/` — ~400 LOC)
- Report pipeline (`report_pipeline.rs` — ~850 LOC)
- Callgraph / CFG / SCC analysis (~600 LOC)
- Capability consumers (~300 LOC)

If any of these are consumed independently, splitting into `canon-smt`, `canon-query`
would reduce compile times and clarify boundaries. Defer until crate boundaries are
stable.

---

## 15. `agent_consumer.rs` — split into focused files

At 1172 lines it covers: graph scheduling, executor delta dispatch, graph patch application,
LLM response parsing, report writing, snapshot persistence, and stall detection.

Split along seams:
- `consumers/agent/scheduler.rs` — `schedule_next`, `plan_if_stalled`, `seed_orchestration`
- `consumers/agent/executor.rs` — delta dispatch, `parse_executor_deltas`, `delta_to_cap_args`
- `consumers/agent/reports.rs` — `write_graph_report`, `append_llm_response_log`, `safe_filename`
- `consumers/agent/patch.rs` — `extract_graph_patch_from_llm_result`, `apply_graph_patch` wiring
- `consumers/agent/mod.rs` — top-level `AgentConsumer`, `handle`, `AgentWorkerState`

---

## 16. Bootstrap events not recorded for `NodeStarted` etc.

The goal graph projector (`canon-event-store/src/goal_graph_projector.rs`) tries to
match `"node_started"`, `"node_completed"`, `"node_failed"` canon event kinds to update
node status — but those events are never written to the tlog (see item 1).

Fix: write `NodeStarted`, `NodeCompleted`, `NodeFailed` to tlog, then the projector
can derive full node lifecycle from events alone and `agent_state` snapshot blobs can
be removed.

---

## 17. `agent_state` blob events — deprecate

`RuntimeEvent::AgentState { payload }` writes the entire `GoalGraph` as a JSON blob
to the tlog on every persist. This is the anti-pattern we replaced with event sourcing.

Once item 16 is done (NodeStarted/Completed/Failed in tlog), `agent_state` can be
removed. The projector will reconstruct graph state from fine-grained events instead.

---

## 18. `reports_out/` — tool_result not yet wired ✅

Added `"tool_result"` arm to `replay_capability_graph_from_tlog`: updates node status
to `completed`/`failed`, stores `output` in `CapabilityOpNode::result`, and computes
`duration_ms` from the tool_call start time.

---

## Notes

- Items 1, 16, 17 are coupled: fix them together in one pass.
- Items 4 and 5 are pure deletes — zero risk, immediate LOC reduction.
- Items 3 and 9 require agreeing on a canonical representation before touching call sites.
- Estimated total LOC reduction: **1500–2500 lines** across all items.

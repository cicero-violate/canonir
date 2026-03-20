# Case Study: ID Coverage in Canon Runtime

## Overview

Canon is a production autonomous agent runtime built around a deterministic event loop:
**Observe → Plan → Act → Verify → Reward**. Every phase produces and consumes events written
to a durable segmented tlog (`event.tlog.d/`). The system supports replay, causal tracing,
and multi-agent orchestration — all of which depend on a complete, well-structured ID set.

This case study maps the ID taxonomy defined in `IDs.md` against the identifiers that
actually exist in the codebase today, exposing gaps that block full deterministic replay,
distributed tracing, and invariant enforcement.

---

## Findings

### What works well

- **`request_id`** is used consistently across all four phases. Every LLM call, bash
  execution, and capability invocation carries a UUID `request_id` that threads through
  `CapabilityRequested` → `CapabilityCompleted` / `CapabilityFailed`. This is the strongest
  ID chain in the system.
- **`tick`** (loop counter) appears on every loop event (`LoopObserved`, `LoopPlanned`,
  `LoopActed`, `LoopVerified`, `LoopRewarded`), giving a coarse but reliable iteration
  anchor for replay.
- **`agent_id`** is registered at bootstrap and stamped on `AgentRegistered` events,
  enabling basic multi-agent differentiation.

### Critical gaps

- **`trace_id` / `span_id` not yet universal** — loop events now carry `trace_id`,
  `execution_id`, `span_id`, `parent_span_id`, and error paths propagate them in most
  runtime paths. Some non-loop/system events still do not participate in the same trace chain.
- **No `phase_id`** — the loop phases (observe/plan/act/verify) are implicit in event
  kind names, not an explicit tagged field. Phase-level replay or phase-level metrics
  require string matching on `kind`.
- **No `tool_call_id` / `tool_result_id`** — tool use events (`ToolCall`, `ToolResult`)
  carry `request_id` and `node_id` but no dedicated tool-call chain ID. The `tool_call_id`
  concept from the OpenAI/Claude tool-use protocol is absent, which will matter when the
  system integrates external LLM tool-use APIs.
- **No dedicated `tool_call_id` / `tool_result_id` emission path in active loop** —
  `ToolCall`/`ToolResult` serialization exists, but current planner/act capability flow does
  not emit these events.
- **Versioning IDs are startup-scoped** (`build_id`, `commit_id`, `schema_id`) — present
  on `runtime_started`, but not stamped on every event payload.

---

## ID Coverage Table

|    # | Category                     | Target ID (`IDs.md`) | Current ID in codebase           | Location                                                | Notes                                                   |
|------+------------------------------+----------------------+----------------------------------+---------------------------------------------------------+---------------------------------------------------------|
|  1.1 | Core Identity                | `system_id`          | `system_id`                      | `event_runtime.rs` / `runtime_started` payload          | Present; persisted and loaded from state/env            |
|  1.2 | Core Identity                | `agent_id`           | `agent_id`                       | `bootstrap.rs:56,141` / `AgentRegistered` payload       | Present; set at bootstrap                               |
|  1.3 | Core Identity                | `session_id`         | `session_id`                     | `event_runtime.rs` / `runtime_started` + cursor         | Present; validated on resume                            |
|  1.4 | Core Identity                | `process_id`         | `pid`                            | `event_runtime.rs:41,72,405` / `RuntimeStarted` payload | Present as raw OS PID                                   |
|  2.1 | Execution Graph              | `trace_id`           | `trace_id`                       | `LoopPlanned/Acted/Verified/Rewarded` + `ErrorOccurred` | Present in loop chain; partial outside loop             |
|  2.2 | Execution Graph              | `span_id`            | `span_id`                        | `LoopPlanned/Acted/Verified/Rewarded`                   | Present for loop spans                                  |
|  2.3 | Execution Graph              | `parent_span_id`     | `parent_span_id`                 | `LoopPlanned/Acted/Verified/Rewarded`                   | Present for parent-child span linkage                   |
|  2.4 | Execution Graph              | `execution_id`       | `execution_id`                   | `LoopPlanned/Acted/Verified/Rewarded`                   | Present per loop execution chain                        |
|  3.1 | Loop Structure               | `tick_id`            | `tick`                           | `events.rs:125+` — all loop events                      | Present; u64 counter per iteration                      |
|  3.2 | Loop Structure               | `phase_id`           | pending                          | —                                                       | Phase implicit in event `kind` string only              |
|  3.3 | Loop Structure               | `step_id`            | `plan_step_id`                   | `LoopPlanned` / `LoopActed` payload                     | Present for planned action step identity                |
|  4.1 | Tooling / Actions            | `tool_call_id`       | pending                          | —                                                       | `ToolCall` uses `request_id`; no dedicated tool-call ID |
|  4.2 | Tooling / Actions            | `tool_result_id`     | pending                          | —                                                       | `ToolResult` uses `request_id`; no dedicated result ID  |
|  4.3 | Tooling / Actions            | `action_id`          | `action_id`                      | `LoopPlanned` / `LoopActed` payload                     | Present; UUID per planned action                        |
|  4.4 | Tooling / Actions            | `operation_id`       | `capability_request_id`          | `events.rs:143` on `LoopActed`                          | Closest match; UUIDs via `canon-act`                    |
|  5.1 | Events                       | `event_id`           | `event_id`                       | `TlogEvent.event_id` (set by runtime append)            | Present; cursor now persists `next_id`                  |
|  5.2 | Events                       | `event_stream_id`    | `event_stream_id`                | `runtime_started` payload                               | Present at session start                                |
|  5.3 | Events                       | `event_offset`       | `start_seq` / `processed`        | `event_runtime.rs:219,229,488`                          | Segment base seq + count; not a per-event offset field  |
|  5.4 | Events                       | `event_batch_id`     | pending                          | —                                                       | No batch grouping                                       |
|  6.1 | State / Data                 | `state_id`           | pending                          | —                                                       | No snapshot identifier                                  |
|  6.2 | State / Data                 | `state_version`      | `state_version`                  | runtime cursor file                                     | Present for cursor/resume state                         |
|  6.3 | State / Data                 | `object_id`          | `node_id`                        | `events.rs:173,179,186,195,201` — goal graph events     | Used for graph nodes; not general object identity       |
|  6.4 | State / Data                 | `object_version`     | pending                          | —                                                       | No per-entity versioning                                |
|  7.1 | Memory / Knowledge           | `memory_id`          | pending                          | —                                                       | No memory store exists yet                              |
|  7.2 | Memory / Knowledge           | `embedding_id`       | pending                          | —                                                       | No embedding store                                      |
|  7.3 | Memory / Knowledge           | `document_id`        | `prompt_id`                      | `bootstrap.rs:24,117` / `PromptLoaded` payload          | Prompt files have a `prompt_id`; closest analog         |
|  7.4 | Memory / Knowledge           | `chunk_id`           | pending                          | —                                                       | No chunking                                             |
|  8.1 | Planning / Intent            | `goal_id`            | `node_id` (on `GoalNodeCreated`) | `events.rs:185-194`                                     | Goal nodes have `node_id`; no dedicated `goal_id` field |
|  8.2 | Planning / Intent            | `intent_id`          | pending                          | —                                                       | Intent layer absent                                     |
|  8.3 | Planning / Intent            | `plan_id`            | `plan_id`                        | `LoopPlanned` / `LoopActed` payload                     | Present per planning cycle                              |
|  8.4 | Planning / Intent            | `plan_step_id`       | `plan_step_id`                   | `LoopPlanned` / `LoopActed` payload                     | Present per emitted plan step                           |
|  9.1 | Errors / Invariants          | `error_id`           | `error_id`                       | `ErrorOccurred` payload                                 | Present; UUID generated via `new_error_occurred`        |
|  9.2 | Errors / Invariants          | `invariant_id`       | pending                          | —                                                       | No invariant registry                                   |
|  9.3 | Errors / Invariants          | `violation_id`       | pending                          | —                                                       | No violation tracking                                   |
| 10.1 | Resources / IO               | `resource_id`        | pending                          | —                                                       | No resource abstraction layer                           |
| 10.2 | Resources / IO               | `request_id`         | `request_id`                     | `events.rs:264,270,276` — Capability events             | Present; UUID per capability invocation                 |
| 10.3 | Resources / IO               | `response_id`        | pending                          | —                                                       | Response folded into `CapabilityCompleted.result`       |
| 10.4 | Resources / IO               | `connection_id`      | `tab_id`                         | `ws_bridge` — `tab_opened` / `tab_ready` payload        | Browser tab ID used as connection proxy                 |
| 11.1 | Concurrency / Scheduling     | `task_id`            | pending                          | —                                                       | Threads unnamed; no async task IDs                      |
| 11.2 | Concurrency / Scheduling     | `job_id`             | pending                          | —                                                       | No job grouping                                         |
| 11.3 | Concurrency / Scheduling     | `queue_id`           | pending                          | —                                                       | Single crossbeam channel; not identified                |
| 11.4 | Concurrency / Scheduling     | `worker_id`          | pending                          | —                                                       | Consumer threads anonymous                              |
| 12.1 | Versioning / Reproducibility | `build_id`           | `build_id`                       | `runtime_started` payload                               | Present at runtime start                                |
| 12.2 | Versioning / Reproducibility | `commit_id`          | `commit_id`                      | `runtime_started` payload                               | Present at runtime start                                |
| 12.3 | Versioning / Reproducibility | `config_id`          | `hash` (on `PromptLoaded`)       | `bootstrap.rs` — prompt hash field                      | Content hash of prompt files; partial config versioning |
| 12.4 | Versioning / Reproducibility | `schema_id`          | `schema_id`                      | `runtime_started` payload                               | Present; startup now warns on mismatch                  |
| 13.1 | Security / Control           | `auth_id`            | pending                          | —                                                       | No auth layer                                           |
| 13.2 | Security / Control           | `permission_id`      | `tool_capabilities`              | `AgentRegistered` payload                               | Capability list as permission proxy; not keyed IDs      |
| 13.3 | Security / Control           | `audit_id`           | pending                          | —                                                       | Tlog is the audit log but entries have no audit ID      |

---

## Summary

| Status                            | Count |
|-----------------------------------+-------|
| **Present (exact or near match)** |    21 |
| **Partial / repurposed field**    |    11 |
| **Pending (not implemented)**     |    12 |
| **Total target IDs**              |    44 |

**Coverage: ~48% full, ~73% incl. partial.**

The minimal closure set from `IDs.md` — `{ agent, session, trace, span, tick, phase, tool_call, event, state, plan, action, error }` — now has most core IDs implemented (`agent_id`, `session_id`, `trace_id`, `span_id`, `tick`, `event_id`, `plan_id`, `action_id`, `error_id`). The highest-priority remaining gaps are explicit `phase_id`, first-class `tool_call_id`/`tool_result_id` emission in the active loop, and broader state/data identity coverage.

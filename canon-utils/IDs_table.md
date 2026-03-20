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

- **No `trace_id` / `span_id` propagation** — `trace_id` exists on `ErrorOccurred` but
  is always `null` in practice. No span envelope links an LLM call to its resulting bash
  command to the verify result. Cross-phase causality cannot be reconstructed from the log.
- **No `session_id`** — there is no envelope that groups all events belonging to one
  runtime invocation. If the process restarts, events from two runs are indistinguishable
  without parsing `runtime_started` timestamps manually.
- **No `phase_id`** — the loop phases (observe/plan/act/verify) are implicit in event
  kind names, not an explicit tagged field. Phase-level replay or phase-level metrics
  require string matching on `kind`.
- **No `tool_call_id` / `tool_result_id`** — tool use events (`ToolCall`, `ToolResult`)
  carry `request_id` and `node_id` but no dedicated tool-call chain ID. The `tool_call_id`
  concept from the OpenAI/Claude tool-use protocol is absent, which will matter when the
  system integrates external LLM tool-use APIs.
- **No `error_id`** — `ErrorOccurred` events have no unique ID of their own. The same
  logical error is re-emitted multiple times (observed 7 duplicate `error_occurred` events
  for a single `cargo new` failure in the tlog), and there is no way to deduplicate or
  correlate them.
- **No versioning IDs** (`build_id`, `commit_id`, `schema_id`) — the tlog contains no
  record of which binary version or git commit produced the events, making cross-version
  replay unsafe.

---

## ID Coverage Table

|    # | Category                     | Target ID (`IDs.md`) | Current ID in codebase           | Location                                                | Notes                                                   |
|------+------------------------------+----------------------+----------------------------------+---------------------------------------------------------+---------------------------------------------------------|
|  1.1 | Core Identity                | `system_id`          | pending                          | —                                                       | No global instance ID exists                            |
|  1.2 | Core Identity                | `agent_id`           | `agent_id`                       | `bootstrap.rs:56,141` / `AgentRegistered` payload       | Present; set at bootstrap                               |
|  1.3 | Core Identity                | `session_id`         | pending                          | —                                                       | No per-invocation session envelope                      |
|  1.4 | Core Identity                | `process_id`         | `pid`                            | `event_runtime.rs:41,72,405` / `RuntimeStarted` payload | Present as raw OS PID                                   |
|  2.1 | Execution Graph              | `trace_id`           | `trace_id`                       | `events.rs:123` on `ErrorOccurred`                      | Field exists but always `null` — not propagated         |
|  2.2 | Execution Graph              | `span_id`            | pending                          | —                                                       | No span envelope on any event                           |
|  2.3 | Execution Graph              | `parent_span_id`     | pending                          | —                                                       | No causal chain linking                                 |
|  2.4 | Execution Graph              | `execution_id`       | pending                          | —                                                       | No single-run execution ID                              |
|  3.1 | Loop Structure               | `tick_id`            | `tick`                           | `events.rs:125+` — all loop events                      | Present; u64 counter per iteration                      |
|  3.2 | Loop Structure               | `phase_id`           | pending                          | —                                                       | Phase implicit in event `kind` string only              |
|  3.3 | Loop Structure               | `step_id`            | pending                          | —                                                       | No atomic step ID within a phase                        |
|  4.1 | Tooling / Actions            | `tool_call_id`       | pending                          | —                                                       | `ToolCall` uses `request_id`; no dedicated tool-call ID |
|  4.2 | Tooling / Actions            | `tool_result_id`     | pending                          | —                                                       | `ToolResult` uses `request_id`; no dedicated result ID  |
|  4.3 | Tooling / Actions            | `action_id`          | `action_kind`                    | `events.rs:135` on `LoopPlanned` / `LoopActed`          | Kind string, not a unique action ID                     |
|  4.4 | Tooling / Actions            | `operation_id`       | `capability_request_id`          | `events.rs:143` on `LoopActed`                          | Closest match; UUIDs via `canon-act`                    |
|  5.1 | Events                       | `event_id`           | `id` / `next_id`                 | `lib.rs:62,259` in `EventRuntime`                       | Internal u64 counter; not stamped on emitted events     |
|  5.2 | Events                       | `event_stream_id`    | pending                          | —                                                       | Tlog path used as implicit stream identity              |
|  5.3 | Events                       | `event_offset`       | `start_seq` / `processed`        | `event_runtime.rs:219,229,488`                          | Segment base seq + count; not a per-event offset field  |
|  5.4 | Events                       | `event_batch_id`     | pending                          | —                                                       | No batch grouping                                       |
|  6.1 | State / Data                 | `state_id`           | pending                          | —                                                       | No snapshot identifier                                  |
|  6.2 | State / Data                 | `state_version`      | pending                          | —                                                       | No monotonic state version                              |
|  6.3 | State / Data                 | `object_id`          | `node_id`                        | `events.rs:173,179,186,195,201` — goal graph events     | Used for graph nodes; not general object identity       |
|  6.4 | State / Data                 | `object_version`     | pending                          | —                                                       | No per-entity versioning                                |
|  7.1 | Memory / Knowledge           | `memory_id`          | pending                          | —                                                       | No memory store exists yet                              |
|  7.2 | Memory / Knowledge           | `embedding_id`       | pending                          | —                                                       | No embedding store                                      |
|  7.3 | Memory / Knowledge           | `document_id`        | `prompt_id`                      | `bootstrap.rs:24,117` / `PromptLoaded` payload          | Prompt files have a `prompt_id`; closest analog         |
|  7.4 | Memory / Knowledge           | `chunk_id`           | pending                          | —                                                       | No chunking                                             |
|  8.1 | Planning / Intent            | `goal_id`            | `node_id` (on `GoalNodeCreated`) | `events.rs:185-194`                                     | Goal nodes have `node_id`; no dedicated `goal_id` field |
|  8.2 | Planning / Intent            | `intent_id`          | pending                          | —                                                       | Intent layer absent                                     |
|  8.3 | Planning / Intent            | `plan_id`            | `llm_request_id`                 | `events.rs:138` on `LoopPlanned`                        | LLM call ID used as plan proxy; not a true plan ID      |
|  8.4 | Planning / Intent            | `plan_step_id`       | pending                          | —                                                       | Steps implicit in `LoopPlanned` ordering                |
|  9.1 | Errors / Invariants          | `error_id`           | pending                          | —                                                       | `ErrorOccurred` has no unique ID; duplicates observed   |
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
| 12.1 | Versioning / Reproducibility | `build_id`           | pending                          | —                                                       | Not stamped in tlog                                     |
| 12.2 | Versioning / Reproducibility | `commit_id`          | pending                          | —                                                       | Not stamped in tlog                                     |
| 12.3 | Versioning / Reproducibility | `config_id`          | `hash` (on `PromptLoaded`)       | `bootstrap.rs` — prompt hash field                      | Content hash of prompt files; partial config versioning |
| 12.4 | Versioning / Reproducibility | `schema_id`          | pending                          | —                                                       | No event schema version in tlog                         |
| 13.1 | Security / Control           | `auth_id`            | pending                          | —                                                       | No auth layer                                           |
| 13.2 | Security / Control           | `permission_id`      | `tool_capabilities`              | `AgentRegistered` payload                               | Capability list as permission proxy; not keyed IDs      |
| 13.3 | Security / Control           | `audit_id`           | pending                          | —                                                       | Tlog is the audit log but entries have no audit ID      |

---

## Summary

| Status                            | Count |
|-----------------------------------+-------|
| **Present (exact or near match)** |    11 |
| **Partial / repurposed field**    |    11 |
| **Pending (not implemented)**     |    22 |
| **Total target IDs**              |    44 |

**Coverage: ~25% full, ~50% partial.**

The minimal closure set from `IDs.md` — `{ agent, session, trace, span, tick, phase, tool_call, event, state, plan, action, error }` — has 4 of 12 properly implemented (`agent_id`, `tick`, `request_id` as action, `request_id` as event proxy). The eight gaps in the minimal set are the highest-priority items to address before the system can support deterministic replay or distributed execution.

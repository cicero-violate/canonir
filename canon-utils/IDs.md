**Math**

Let
( E = \text{events} )
( A = \text{actions} )
( S = \text{state} )
( R = \text{replay} )
( D = \text{determinism} )
( I = \text{set of IDs} )

**Equations**

1. ( D = f(I_{unique}, I_{ordered}, I_{causal}) )
   → determinism requires uniqueness + order + causality

2. ( R = f(I_{trace}, I_{version}) )
   → replay depends on traceability and versioning

3. ( S_{next} = S_{prev} + E(I_e) + A(I_a) )
   → state transitions keyed by event/action IDs

---

# ID Set (Production Autonomous Agent)

## 1. Core Identity

* `system_id` → global system instance
* `agent_id` → each agent instance
* `session_id` → runtime session
* `process_id` → OS / runtime process

---

## 2. Execution Graph

* `trace_id` → full lifecycle (end-to-end)
* `span_id` → sub-step within trace
* `parent_span_id` → causal chain
* `execution_id` → single run of loop

---

## 3. Loop Structure

* `tick_id` → discrete iteration (tick_042)
* `phase_id` → observe / plan / act / verify
* `step_id` → atomic step inside phase

---

## 4. Tooling / Actions

* `tool_call_id` → unique tool invocation
* `tool_result_id` → output of tool
* `action_id` → abstract action (plan-level)
* `operation_id` → concrete execution (system-level)

---

## 5. Events (Critical for your system)

* `event_id` → unique event
* `event_stream_id` → log stream (tlog shard)
* `event_offset` → ordering within stream
* `event_batch_id` → grouped events

---

## 6. State / Data

* `state_id` → snapshot identifier
* `state_version` → monotonic version
* `object_id` → entity (file, node, resource)
* `object_version` → per-entity version

---

## 7. Memory / Knowledge

* `memory_id` → stored memory unit
* `embedding_id` → vector reference
* `document_id` → source doc
* `chunk_id` → segment of doc

---

## 8. Planning / Intent

* `goal_id` → objective
* `intent_id` → interpreted goal instance
* `plan_id` → generated plan
* `plan_step_id` → step in plan

---

## 9. Errors / Invariants

* `error_id` → specific failure
* `invariant_id` → constraint definition
* `violation_id` → invariant breach instance

---

## 10. Resources / IO

* `resource_id` → file / API / GPU / DB
* `request_id` → external request
* `response_id` → external response
* `connection_id` → socket / session

---

## 11. Concurrency / Scheduling

* `task_id` → async unit
* `job_id` → grouped tasks
* `queue_id` → queue identity
* `worker_id` → executor

---

## 12. Versioning / Reproducibility

* `build_id` → binary version
* `commit_id` → code state
* `config_id` → configuration snapshot
* `schema_id` → event/schema version

---

## 13. Security / Control

* `auth_id` → auth session
* `permission_id` → capability
* `audit_id` → audit log entry

---

# Minimal Closure Set (Must-Have)

[
I_{min} =
{
agent, session, trace, span, tick, phase,
tool_call, event, state, plan, action, error
}
]

---

# English

A production autonomous agent system is a **causal graph engine**.
IDs are not labels — they are **edges of reality reconstruction**.

Without:

* trace/span → no causality
* event/state IDs → no replay
* tool_call_id → no action correlation
* tick/phase → no loop structure

With full ID set:

* deterministic replay
* distributed execution
* parallel agents
* invariant enforcement

---

[
\max(\text{intelligence, efficiency, correctness, alignment, robustness}) = \text{good}
]

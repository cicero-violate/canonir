# FEATURES.md — Claude Code Patterns to Implement in Canon

**Sources**:
- `github.com/anthropics/claude-code` (public repo, plugins + docs)
- `@anthropic-ai/claude-code@1.0.128` npm package (`cli.js` pretty-printed, 447k lines)
- `github.com/openai/codex` — `codex-rs/` Rust workspace (65 crates, open source)

**Internal codename (Claude Code)**: `tengu`
**Status**: Reference document. Each section grades feasibility and maps to canon components.

---

## Appendix A — Internals from npm Source (`cli.js` v1.0.128)

These are observations from the actual pretty-printed JS source, not documentation.
Line numbers reference `/workspace/git_repos/claude-code-src/cli-pretty.js`.

### A.1 Main Agent Loop (`vO`, ~line 420054)

The core loop is a `while (O)` with a recursive tail call:

```
while (O):
  O = false
  for await message of X01(messages, options):   // streaming LLM call
    collect into C (assistant messages)

  T = all tool_use blocks from C
  if T is empty:
    run stop-hook path (Nh6, Eh6)
    return

  for await result of Lh6(T, C, ...):            // parallel tool dispatch
    collect tool results into P

  if aborted: return
  if preventContinuation: return

  // drain message queue (injected steering messages)
  for await msg of d01(null, context, null, queue, ...):
    collect into c

  // check for model fallback (Opus limit hit)
  if XFB(mainLoopModel): switch to fallback model

  // recurse with extended history
  yield* vO({ messages: [...K, ...C, ...P], ... })
```

**Key insight**: The agent loop is a recursive generator (`yield*`), not a flat while-loop.
Each turn extends the message history and recurses. Tool results are fed back as `user`-role
messages (the Anthropic API's `tool_result` content block), which the next LLM call sees.

**Canon analogue**: `PlanConsumer` dispatches one LLM call per shape tick. There is no
recursive turn structure — each execute tick is a separate event. The multi-turn conversation
history is not maintained across ticks in the same way.

---

### A.2 Hook System (~line 352220)

**Registered hook event types** (exact enum):
```
PreToolUse, PostToolUse, Notification, UserPromptSubmit,
SessionStart, SessionEnd, Stop, SubagentStop, PreCompact
```

**PreToolUse flow** (~line 420453):
1. Run all matching `PreToolUse` hooks as an async generator (`q7A`)
2. Collect: `blockingErrors`, `preventContinuation`, `stopReason`, `permissionBehavior`
3. If any hook sets `permissionBehavior = "allow"` → bypass permission check entirely
4. If `permissionBehavior = "deny"` → immediately return `tool_result` with `is_error: true`
5. If no hook decision → fall through to normal permission check (`G(A, J, Z, Y, B, hint)`)
6. If blocked: yield `{ type: "tool_result", content: stopReason, is_error: true }`

**Stop hook** (used by Ralph Wiggum, ~line 359778):
```js
hook_event_name: subagentStop ? "SubagentStop" : "Stop"
```
Output `{ "decision": "block", "reason": "<prompt>" }` to prevent exit and re-feed the prompt.

**PostToolUse** (~line 420708): Can `suppressOutput` (hide tool result from user display)
and inject a custom `systemMessage` into the conversation.

**Canon analogue**: No hook system. `ActConsumer` executes and emits `LoopActed`; there is
no interception layer before or after. The Pre-execution destructive command block (Feature 5
in main body) is the equivalent of a `PreToolUse` hook.

---

### A.3 Permission Modes (~line 352231)

Four modes (exact strings): `"default"`, `"plan"`, `"acceptEdits"`, `"bypassPermissions"`

- `default`: asks for permission on each tool use
- `plan`: UI shows ⏸ pause indicator; no tool execution (plan-only mode)
- `acceptEdits`: auto-accepts file edits without prompting (⏵⏵)
- `bypassPermissions`: skips all permission checks (⏵⏵, shown in error color)

The permission mode is read at each tool call: `(await Z.getAppState()).toolPermissionContext.mode`

---

### A.4 Sub-Agent Tool (`cA1`, ~line 432321)

The `Agent` tool (internal name `b7`) implementation:

```js
async *call({ prompt, subagent_type }, toolUseContext, canUseTool, parentMessage):
  W = activeAgents.find(a => a.agentType === subagent_type)
  if !W: throw "Agent type not found"

  for await msg of gu1({
    agentDefinition: W,
    promptMessages: [userMessage(prompt)],
    toolUseContext,
    canUseTool,
    isAsync: false,
    recordMessagesToSessionStorage: true,
  }):
    yield progress events for each tool_use/tool_result

  lastAssistant = last assistant message in collected msgs
  yield { type: "result", data: { content: lastAssistant.text, totalDurationMs, totalTokens } }
```

**Key observations**:
- `isAsync: false` — sub-agents are synchronous (block the parent until done)
- The result surfaced to the parent is only the **text content** of the last assistant message
- Tool use activity inside the sub-agent is emitted as `progress` events visible to the parent
- `recordMessagesToSessionStorage: true` — sub-agent conversation is stored in session

**Canon analogue**: No sub-agent mechanism. Each `PlanConsumer` LLM call is a single-turn
call. Implementing parallel multi-agent planning (Feature 2) would mirror `gu1()` but with
concurrent dispatch.

---

### A.5 TodoWrite / TodoRead (~line 385234)

```js
// TodoWrite
async *call({ todos }, context):
  old = appState.todos[agentId] ?? []
  new = todos.every(t => t.status === "completed") ? [] : todos
  setAppState(s => ({ ...s, todos: { ...s.todos, [agentId]: new } }))
  yield { type: "result", data: { oldTodos: old, newTodos: todos } }

// tool_result always says:
"Todos have been modified successfully. Ensure that you continue to use
the todo list to track your progress."
```

**Key observations**:
- Todo lists are **scoped per `agentId`** — sub-agents have independent todo lists
- If all todos are `completed`, the list is cleared to `[]` automatically
- The tool result message is **fixed** — the LLM always gets the same confirmation string
- Reminder injection (~line 397030): if `turnsSinceLastTodoWrite > threshold`, a system
  reminder is injected: *"The TodoWrite tool hasn't been used recently..."*

**Canon analogue**: No todo system. The planner's `batch_acted` list serves a similar
function (tracking what was done), but it is not structured or surfaced to the LLM as a
persistent task list.

---

### A.6 Auto-Compact / Conversation Summarisation (~line 442601)

```js
async function pt5(messages):
  // fit as many trailing messages as possible within token budget (ct5())
  // working backwards from most recent

  title = await callLLM({
    systemPrompt: [summarizeSystemPrompt],
    userPrompt: "Please write a 5-10 word title for the following conversation:\n\n" + formatted,
    promptCategory: "summarize_convo",
    enablePromptCaching: true,
  })
  return title
```

**Key observations**:
- Compaction uses a **separate LLM call** with `promptCategory: "summarize_convo"` (not the
  main model call — allows billing/routing separation)
- The summary is just a **5–10 word title**, not a full transcript summary — the actual
  compaction strategy replaces old messages with a system-injected summary block
- Token budget is computed by `ct5()` — the function that reads the context window limit
- `autoCompactEnabled` is a user-configurable boolean (default: `true`)
- `PreCompact` hook fires before compaction — hooks can inspect or block it

**Canon analogue**: No context window management. The tlog is unbounded and the planner
prompt grows with each batch. Long-running sessions will eventually hit token limits.
The natural equivalent would be summarising `batch_acted` when it exceeds N entries.

---

### A.7 Model Fallback (~line 420087)

```js
catch (j):
  if j instanceof RateLimitError && fallbackModel:
    model = fallbackModel
    O = true          // re-run the while loop with new model
    yield warning("Model fallback triggered: switching from X to Y")
    continue
```

Claude Code automatically falls back to a secondary model when the primary hits rate limits.
The fallback model is configured separately from `mainLoopModel`. The entire turn is retried
with the fallback model — prior assistant messages from the failed attempt are discarded
(`C.length = 0`).

**Canon analogue**: `LlmCapabilityHandler` has no fallback. A failed LLM call causes
`CapabilityFailed`, which `PlanConsumer` handles by emitting `no_op`. There is no retry
with an alternate model.

---

## 1. Confidence-Based Issue Filtering

**Source**: `plugins/code-review`

### How Claude Code Does It

Each review agent independently scores every finding from 0–100 before it is acted on:

```
0   = not confident, false positive
25  = might be real
50  = real but minor
75  = highly confident
100 = absolutely certain
```

Issues below the threshold (default 80) are silently dropped. The scoring step is separate
from the detection step and runs *after* the finding is made. For CLAUDE.md compliance
specifically, the scorer also verifies that the guideline explicitly states the violated rule.

### Canon Equivalent

Verify (`canon-verify`) is binary: `passed = compiler_clean && tlog_clean && file_ok`.
There is no graduated confidence model — a minor tlog warning carries the same weight as a
compiler error.

### What to Implement

Add a `severity_score: u8` field (0–100) to `LoopVerified`. `VerifyConsumer` computes this
from a weighted sum:

| Signal                                 | Weight |
|----------------------------------------+--------|
| compiler error                         |    100 |
| compiler warning                       |     20 |
| tlog `error_occurred`                  |     60 |
| `file_path_not_found` for write action |     80 |
| `file_missing` for patched file        |     70 |
| skipped action (batch abort)           |      0 |

`passed` remains the binary gate. `severity_score` is logged to the tlog and surfaced in the
planner prompt's "Action Results" section so the LLM can distinguish a cosmetic warning from a
hard failure. The routing gate can use a low-severity `passed=false` to route `validate` rather
than `shape`, avoiding a full replan for a minor issue.

**Files**: `canon-utils/canon-verify/src/lib.rs`, `canon-utils/canon-event/src/lib.rs`
(add field to `LoopVerified`)

---

## 2. Parallel Multi-Agent Dispatch

**Source**: `plugins/code-review`, `plugins/feature-dev`

### How Claude Code Does It

Both plugins launch multiple LLM agents **simultaneously** for the same artifact:

- `code-review`: 4 parallel agents (2× CLAUDE.md compliance, 1× bug detection, 1× git history)
- `feature-dev`: 2–3 `code-explorer` agents in parallel (each explores a different angle),
  then 3 `code-architect` agents, then 3 `code-reviewer` agents

Results are collected, deduplicated, and merged before the next phase begins.

### Canon Equivalent

`PlanConsumer` dispatches a single LLM call per shape tick. One perspective, one plan.
If the LLM hallucinates (e.g. picks `cargo new` on an existing directory), there is no
second agent to catch it.

### What to Implement

**Multi-perspective planning**: On a `route_selected=shape` event, dispatch N concurrent
`llm.call` capability requests (N=2 initially) with different role frames:

- **Planner A** (`role=planner_conservative`): emphasises smallest viable change, prefers
  existing scaffolding.
- **Planner B** (`role=planner_generative`): emphasises output volume, prefers writing whole
  modules.

Both emit `CapabilityRequested`. `PlanConsumer` waits for both completions (via two tracked
`plan_tool_call_id` values). It then applies a merge policy:

1. If both agree on action kind → take the higher-LOC plan.
2. If they disagree → prefer the plan whose first action does not overlap with a known failure
   in `batch_acted`.

This requires `PendingPlan` to support a `sibling_request_id: Option<String>`.

**Files**: `canon-utils/canon-plan/src/lib.rs`

---

## 3. Stop-Hook / Completion-Promise Loop (Ralph Wiggum Pattern)

**Source**: `plugins/ralph-wiggum`

### How Claude Code Does It

A **Stop hook** runs whenever Claude attempts to exit. The hook:

1. Reads a state file (`.claude/ralph-loop.local.md`) to check if a loop is active.
2. Checks whether the agent's last output contains a `<promise>TEXT</promise>` tag matching
   the configured completion promise.
3. Checks whether `max_iterations` has been reached.
4. If neither exit condition is met: outputs `{"decision": "block", "reason": <prompt>}`,
   which causes Claude Code to feed the original prompt back as a new input rather than
   terminating.

The same prompt is fed on every iteration. The agent observes its own prior file changes via
the filesystem.

State file format (markdown frontmatter + body):
```markdown
---
iteration: 7
max_iterations: 50
completion_promise: "DONE"
---
<prompt text>
```

### Canon Equivalent

The canon event runtime loops via P3 (1-second tick timer) and the routing gate, but there is
no explicit completion-promise mechanism. `finish_ready` is set when `evaluate_goal_satisfied`
returns `true`, but there is no agent-declared signal that the agent itself considers the work
done (vs. the runtime checking externally).

### What to Implement

**Completion-promise in LoopActed / LlmAction**:

Add `LlmAction::Done { reason }` (already exists in `PlanConsumer`) behaviour:
- When `Done` is parsed, set `last_done_goal = current goal text` (already done).
- Emit a new `CanonEvent::LoopFinished { reason, iteration }` event.
- In `update_route_runtime_state`, set `finish_ready = true` when `LoopFinished` is received
  **and** `evaluate_goal_satisfied` confirms the external criteria are met.

**Max-iteration guard**:

Add `max_shape_ticks: Option<u64>` to `RouteRuntimeState`. Increment a `shape_tick_count`
counter each time `route_selected=shape` fires. If `shape_tick_count >= max_shape_ticks`,
emit a `LoopFinished { reason: "max_iterations_reached" }` and halt, rather than looping
silently forever (which is the current failure mode exposed in the tlog analysis).

**Files**: `canon-utils/canon-runtime/src/bin/event_runtime.rs`,
`canon-utils/canon-event/src/lib.rs` (new `LoopFinished` event)

---

## 4. Structured N-Phase Workflow with Human Gate Points

**Source**: `plugins/feature-dev`

### How Claude Code Does It

The `feature-dev` plugin enforces 7 explicit phases, each with defined inputs/outputs and
**explicit human approval gates** before phase transition:

| Phase                | Gate                               |
|----------------------+------------------------------------|
| Discovery            | Confirm understanding with user    |
| Exploration          | Automatic (agent gathers data)     |
| Clarifying Questions | **Wait for user answers**          |
| Architecture Design  | **Wait for user to pick approach** |
| Implementation       | **Wait for explicit approval**     |
| Quality Review       | **Present issues, ask what to do** |
| Summary              | Automatic                          |

Phases 3, 4, and 5 are hard stops — the workflow cannot advance without user input.

### Canon Equivalent

Canon has route kinds (`Scan/Shape/Execute/Validate/Conclude`) but no concept of numbered
phases within a single goal session. The planner emits actions independently of any structured
progression, so the LLM can oscillate between setup and implementation without a stable phase
context.

### What to Implement

**Phase tracking in the goal spec**:

`GoalSpec` (parsed from `AGENT_GOAL.md`) already has a `requirements` list. Add optional
`phases: Vec<GoalPhase>` where `GoalPhase = { name, criteria: Vec<String> }`. Phases are
defined in the markdown like:

```markdown
## Phases
1. Scaffold: Cargo.toml and src/main.rs exist
2. Core: All required functions implemented
3. Polish: README.md non-empty, cargo build clean
```

**Phase signal in `LoopObserved`**:

`ObserveConsumer` evaluates which phase criteria are satisfied and emits
`current_phase: usize` in `LoopObserved`. The planner prompt's `## Active Goal` section
includes:

```
Current phase: 1 (Scaffold) — completed
Next phase: 2 (Core) — in progress
```

This gives the LLM a stable progress frame, preventing it from re-scaffolding when the
workspace already has a valid `Cargo.toml`.

**Files**: `canon-utils/canon-goal/src/lib.rs`, `canon-utils/canon-observe/src/lib.rs`,
`canon-utils/canon-plan/src/lib.rs` (`build_prompt`)

---

## 5. PreToolUse Security Hook

**Source**: `plugins/security-guidance`

### How Claude Code Does It

A Python `PreToolUse` hook runs before every tool call that writes or executes. It inspects
the tool call payload for 9 patterns:

1. Command injection (`; `, `&& `, `| `, `` ` ``)
2. XSS (`innerHTML`, `document.write`, `eval(`)
3. SQL injection (unparameterised string concatenation near DB calls)
4. `eval()` usage in any language
5. Dangerous HTML (`<script>`, `<iframe>`)
6. Pickle deserialization (`pickle.loads`)
7. `os.system()` calls
8. Hardcoded secrets (regex for `api_key =`, `password =`, `SECRET`)
9. `rm -rf` or similar destructive shell patterns

If a pattern fires, the hook emits a warning (does **not** block by default) so the developer
is reminded before proceeding.

### Canon Equivalent

`ActConsumer` in `canon-act` executes tool calls but has no pre-execution inspection. The
only safety rail is the planner's execution policy text: "Do NOT emit destructive commands"
— which is instruction, not enforcement.

### What to Implement

**Pre-execution gate in `ActConsumer`**:

Before dispatching a `run_command` capability, `ActConsumer` checks the `cmd` field against
a deny-list:

```
BLOCK:  rm -rf, git reset --hard, git clean -f, dd if=, mkfs, shred, >/dev/sd
WARN:   git push --force, cargo clean, truncate
```

For blocked commands: emit `LoopActed { success: false, stderr: "blocked:destructive_command",
exit_code: Some(-1) }` without executing. This is the same signal shape as a skipped action
(Repair 1), but with a different sentinel value so the routing state can distinguish
"we blocked it" from "it was skipped by batch abort".

For warned commands: emit a `Debug` event with kind `"pre_exec_warning"` containing the
pattern matched, then proceed.

**Files**: `canon-utils/canon-act/src/lib.rs` (or equivalent act consumer)

---

## 6. Architectural Pluralism (Multi-Approach Design Before Commit)

**Source**: `plugins/feature-dev` Phase 4

### How Claude Code Does It

Before implementation, `code-architect` generates 3 distinct designs:

- **Minimal**: Fewest changed files, maximum reuse of existing structures.
- **Clean**: Optimal separation of concerns, new abstractions where justified.
- **Pragmatic**: Balanced — the architect's recommended pick.

The designs are presented with explicit trade-off tables. The user selects one. Only after
selection does Phase 5 (implementation) begin.

### Canon Equivalent

The planner emits a single plan per LLM call. The LLM has no mechanism to surface alternative
approaches or flag uncertainty between strategies. If the LLM picks a bad approach (as with
`cargo new` vs. `cargo init`), it will repeat it until something external breaks the loop.

### What to Implement

**Approach-tagging in the planner prompt**:

When `batch_acted` contains one or more failed actions, add to the prompt:

```
## Replanning Context
The previous approach failed. Generate ONE alternative action that takes a
DIFFERENT approach than: "{last_failed_action_kind}: {last_failed_cmd}".
Do not retry the same command with the same arguments.
```

This is a lightweight version of pluralism — rather than generating 3 full designs, the
planner is instructed that the previously-attempted path is prohibited, forcing divergence.

**Full architectural pluralism** (longer term):

On the first `route_selected=shape` after `LoopObserved` (no prior `batch_acted`), dispatch
two concurrent planning LLM calls (see Feature 2), one with `prefer_minimal=true` context and
one without. After both complete, `PlanConsumer` picks the plan whose first command is a
`write_file` (direct, no scaffolding dependency) if the target directory already exists, or
`run_command` with `cargo init` (not `cargo new`) if it exists but has no `Cargo.toml`.

**Files**: `canon-utils/canon-plan/src/lib.rs` (`build_prompt`, `handle_observed`)

---

## 7. Session-State Persistence for Crash Recovery

**Source**: `plugins/ralph-wiggum` (state file), `event_runtime.rs` (cursor file)

### How Claude Code Does It

Ralph stores loop state in `.claude/ralph-loop.local.md` — a human-readable markdown file
with YAML frontmatter. Fields: `iteration`, `max_iterations`, `completion_promise`. On every
iteration, the file is atomically updated (write to `.tmp`, then `mv`). On crash: the next
process restart finds the file and resumes from the last recorded iteration.

### Canon Equivalent

`event_runtime` already has a cursor file (`state/event_runtime.cursor.json`) that persists
`processed`, `start_seq`, `session_id`, and `next_id`. This covers tlog replay position.
What is **not** persisted:

- `last_planned_observed_tick` → lost on restart, so the plan guard resets (this is correct)
- `batch_acted` → lost on restart, so the planner has no context on prior failures
- `shape_tick_count` (proposed in Feature 3) → would also be lost

### What to Implement

**Extended cursor with planning context**:

Add optional fields to `state/event_runtime.cursor.json`:

```json
{
  "last_failed_action_kind": "run_command",
  "last_failed_cmd": "cargo new test_rust_project_v3",
  "shape_tick_count": 7,
  "current_phase": 1
}
```

`RouteRuntimeState` is hydrated from the cursor on startup (same pattern as `resumed_next_id`
at line 686 of `event_runtime.rs`). `save_cursor` is extended to include these fields.

This allows a restarted runtime to know that the previous session already failed at `cargo new`
and to inject that context into the first `build_prompt` call without replaying the full tlog.

**Files**: `canon-utils/canon-runtime/src/bin/event_runtime.rs`
(`load_cursor_state`, `save_cursor`, `RouteRuntimeState`)

---

## 8. Deduplication Guards on Expensive Operations

**Source**: `plugins/code-review` (skips already-reviewed PRs), `canon-verify` (existing
`last_verified_action_key` guard)

### How Claude Code Does It

`code-review` checks before launching agents:

- Is the PR closed? Skip.
- Is the PR a draft? Skip.
- Is the PR trivially automated (e.g. Dependabot)? Skip.
- Does the PR already have a code-review comment from this bot? Skip.

The check runs first, before any expensive LLM calls.

### Canon Equivalent

`VerifyConsumer` already has `last_verified_action_key` (line 57) which deduplicates
verification runs for the same action. `PlanConsumer` has `last_planned_observed_tick` (the
guard that Repair 3 fixes). These are both good examples of the same pattern.

**Gap**: the planner has no guard against replanning with an identical `batch_acted` history.
If the LLM repeatedly emits the same failing command (e.g. `cargo new`), the system will
dispatch identical LLM calls on every shape tick (after Repair 3 is applied).

### What to Implement

**Repeated-failure guard in `PlanConsumer`**:

Track `last_planned_action_signature: Option<String>` — a hash of
`(action_kind, cmd_or_path)` from the last emitted plan. After a failed `LoopActed`:

- If the new plan's first action has the same signature as the last plan → do not dispatch a
  new LLM call. Instead emit `LoopPlanned { action_kind: "no_op", reason: "plan_identical_to_failed" }`.
- Clear `last_planned_action_signature` when a plan **succeeds**.

This prevents the LLM from spinning on the same broken command indefinitely.

**Files**: `canon-utils/canon-plan/src/lib.rs`

---

## 9. Self-Correction via Test Feedback Loop

**Source**: `plugins/feature-dev` Phase 6, `plugins/ralph-wiggum` prompt guidance

### How Claude Code Does It

`feature-dev` Phase 6 launches review agents that produce structured findings with file:line
references and confidence scores. The results are fed back to the planner ("Here are the
issues. Fix now, fix later, or proceed?"). In Ralph, the pattern is simpler: the prompt
instructs the LLM to run tests and fix failures before declaring done.

### Canon Equivalent

`VerifyConsumer` runs `cargo check` and reports errors, but the errors are not fed back into
the planner prompt's next call in a structured way. The planner prompt has:

```
## Compiler Errors (N)
First error: <message>
```

Only the **first** error is shown. There is no file:line linkage. Errors from prior
iterations are not distinguished from new errors introduced by the latest patch.

### What to Implement

**Rich error feed in `build_prompt`**:

When `observed.error_count > 0`, replace the current single-error section with up to 5
errors, each formatted as:

```
### Error 1
file: src/main.rs:42:8
message: expected `}`, found `fn`
context: (3 lines of source around the error, from the file)
```

The context lines are read live from disk using `observed.compiler_errors[i].spans[0]`.

**Delta error tracking**:

`ObserveConsumer` emits both `error_count` and `new_error_count` (errors that were not present
in the previous `LoopObserved`). The planner prompt distinguishes:

```
## Compiler Errors (5 total, 2 new since last action)
```

This allows the LLM to focus only on the errors it introduced, rather than re-attempting to
fix pre-existing issues.

**Files**: `canon-utils/canon-observe/src/lib.rs`, `canon-utils/canon-plan/src/lib.rs`

---

## 10. Workspace-Aware Prompting

**Source**: Failure 4 in `REPAIR_PLANS.md` (derived independently from tlog analysis), also
reflected in `plugins/feature-dev` Phase 2 (codebase exploration before planning)

### How Claude Code Does It

`feature-dev`'s Phase 2 launches `code-explorer` agents to map what already exists before any
architecture is designed. The planner only proposes changes after reading the current state of
relevant files. There is no planning from a blank slate.

### Canon Equivalent

See `REPAIR_PLANS.md` Repair 4. The `build_prompt` function does not include any workspace
state section. This is the immediate fix already planned.

### What to Implement

Already specified in `REPAIR_PLANS.md`. Extended version:

**Directory tree in prompt** (beyond Repair 4 scope):

After the `## Workspace State` section, include a compact recursive tree of the target
project (depth ≤ 3, file sizes omitted, directories suffixed with `/`):

```
test_rust_project_v3/
  Cargo.toml
  src/
    main.rs
    lib.rs
    handlers/
      mod.rs
      user.rs
```

Cap at 40 lines. This gives the LLM a complete mental model of what has already been written,
preventing it from re-writing files that already exist with the correct content.

**Files**: `canon-utils/canon-plan/src/lib.rs` (`build_workspace_state_section`)

---

## Priority Order

|  # | Feature                                  | Effort | Impact   | Prerequisite |
|----+------------------------------------------+--------+----------+--------------|
|  1 | Workspace-Aware Prompting (Repair 4)     | XS     | Critical | —            |
|  2 | Completion-Promise / Max-Iteration Guard | S      | High     | Repair 3     |
|  3 | Rich Error Feed + Delta Errors           | S      | High     | —            |
|  4 | Pre-Execution Destructive Command Block  | S      | High     | —            |
|  5 | Repeated-Failure Deduplication Guard     | S      | High     | Repair 3     |
|  6 | Confidence-Based Verify Scoring          | M      | Medium   | Repair 2     |
|  7 | Replanning Context (Anti-Retry Prompt)   | S      | Medium   | Repair 1,3   |
|  8 | Phase Tracking in Goal + Observed        | M      | Medium   | —            |
|  9 | Extended Cursor (Crash Recovery)         | S      | Medium   | —            |
| 10 | Parallel Multi-Agent Planning            | L      | Low      | Feature 5    |
| 11 | Full Architectural Pluralism             | L      | Low      | Feature 2    |

XS = hours, S = 1 day, M = 2–3 days, L = 1 week

---

## Appendix B — Internals from `codex-rs` (OpenAI Codex, Rust)

**Source**: `/workspace/git_repos/codex/codex-rs` — open-source Rust workspace, 65 crates.
All file paths are relative to `codex-rs/`. Line numbers reference the core crate unless noted.

---

### B.1 Crate Structure (65-crate workspace)

| Crate | Role |
|-------|------|
| `core` | Agent loop, turn execution, tool routing, state — the whole brain |
| `hooks` | AfterAgent / AfterToolUse hook dispatch |
| `exec` / `exec-server` | Process execution and sandboxing |
| `execpolicy` | Rules-engine for command validation |
| `protocol` | Wire types shared across crates |
| `state` | Persistent session state |
| `skills` | Loadable skill modules with MD injection |
| `linux-sandbox` / `process-hardening` | Sandbox backends |
| `mcp-server` | Model Context Protocol server integration |
| `app-server` | HTTP API surface |
| `cli` / `tui` | Frontends |
| `shell-command` | Shell command parsing |
| `file-search` | BM25 semantic file search |

---

### B.2 Main Agent Loop (`core/src/codex.rs`)

**Three nested layers**:

```
submission_loop()  [line ~3122]        ← outer: listens on Submission channel
  └─ run_turn()   [line ~4269]         ← middle: one user turn
       └─ sampling loop [line ~4427]   ← inner: LLM calls until no more tool calls
```

**`submission_loop`**: Sits on a `Receiver<Submission>` channel. Routes by `Op` variant:
`UserInput`, `Interrupt`, `ExecApproval`, `PatchApproval`, `Shutdown`, etc.

**`run_turn`**:
1. Emits `TurnStartedEvent`
2. Runs **pre-sampling compaction** (trim history before first LLM call)
3. Injects skill markdown into prompt
4. Seeds app-connection tools if MCP servers running
5. Records user prompt to `ContextManager`

**Sampling loop** (the equivalent of Claude Code's recursive `vO`):
```
loop:
  build Prompt from ContextManager.for_prompt()
  result = run_sampling_request()         // LLM call with retries
  if result.needs_follow_up:
    continue
  else:
    run AfterAgent hooks
    emit TurnCompletedEvent
    break
```

`run_sampling_request` handles:
- Rate-limit errors → retry with backoff
- Context-window-exceeded → auto-compact then retry
- Stream transport errors → retry up to N times

Inside `try_run_sampling_request` [line ~5455]:
- Creates `ToolCallRuntime` for parallel tool dispatch
- Loops `stream.next()` processing `ResponseEvent` variants:
  - `OutputItemAdded` → create turn item
  - `OutputItemDone` → call `handle_output_item_done()` [line ~5579]
  - `Completed` → sets `needs_follow_up = false`
- Tool calls accumulated in `FuturesOrdered`, awaited in parallel

**Canon analogue**: Canon's `PlanConsumer` → `ActConsumer` → `VerifyConsumer` pipeline
maps roughly to `run_sampling_request` → `ToolCallRuntime` → `AfterAgent` hook. The key
difference: codex keeps the conversation history in memory across tool call rounds within one
turn; canon separates each LLM call into a discrete tlog event.

---

### B.3 Tool System (`core/src/tools/`)

**Two-layer dispatch:**

```
ToolRouter  (router.rs ~line 38)          ← converts ResponseItem → ToolCall
  └─ ToolRegistry (registry.rs ~line 58)  ← maps name → ToolHandler, runs it
```

**`ToolRouter.dispatch_tool_call`** [line ~143]:
- Selects handler type: `LocalShell`, `Function`, `MCP`, `Custom`
- Built-in functions handled directly; MCP tools proxied to MCP server

**`ToolRegistry.dispatch`** [line ~106]:
1. Look up handler by tool name
2. Verify payload type
3. Check if tool is **mutating**
4. If mutating: `tool_call_gate.wait_ready().await` ← **gate blocks until approval granted**
5. Invoke handler, log result via `AfterToolUse` hook

**Built-in tools (exact names)**:

| Tool | What it does |
|------|-------------|
| `local_shell` / `shell` | Shell execution |
| `apply_patch` | Freeform or JSON file patching |
| `update_plan` | Record plan in plan mode |
| `js_repl` / `js_repl_reset` | JavaScript REPL |
| `spawn_agent` | Spawn sub-agent thread |
| `send_input` | Send input to sub-agent |
| `resume_agent` | Resume paused sub-agent |
| `wait` | Wait for sub-agent completion |
| `close_agent` | Terminate sub-agent |
| `search_tool_bm25` | Semantic file search |
| `request_user_input` | Ask user a question mid-turn |
| Dynamic tools | Runtime-registered custom tools |
| MCP tools | From connected MCP servers |

**Parallel vs serial tool execution** (`tools/parallel.rs` ~line 24):
```rust
// non-mutating tools: acquired with read lock → run in parallel
// mutating tools: acquired with write lock → serialised
RwLock controls concurrency
```
Uses `FuturesOrdered` to collect results in dispatch order.

**Tool gate** (`tools/parallel.rs` ~line 81):
```rust
if is_mutating {
    invocation.turn.tool_call_gate.wait_ready().await;
}
```
Mutating tools block until explicit approval signal is received (e.g. user approves a patch).
Non-mutating tools (reads, searches) run immediately in parallel.

**Canon analogue**: Canon's `ActConsumer` executes one planned action at a time — no
parallel tool dispatch, no mutating/non-mutating distinction, no gate mechanism.

---

### B.4 Hook System (`hooks/`)

**Two hook events** (hooks/types.rs ~line 13):

| Hook | Fires when | Payload |
|------|-----------|---------|
| `AfterAgent` | Turn completes | turn_id, input_messages, last_assistant_message |
| `AfterToolUse` | Each tool finishes | tool_name, tool_kind, tool_input, success, duration_ms, sandbox_policy |

`HookResult` variants: `Success`, `FailedContinue`, `FailedAbort`.
`FailedAbort` stops the turn; `FailedContinue` logs and proceeds.

**Notably absent**: No `BeforeToolUse` / `PreToolUse` hook. Approval is handled by the
separate **tool gate** mechanism (B.3) rather than a hook. This is architecturally different
from Claude Code's `PreToolUse` → `allow`/`deny`/`ask` model.

**Canon analogue**: Canon has no hook system. The `LoopVerified` / `LoopActed` events are
the closest equivalent — post-execution signals rather than interception points.

---

### B.5 Context / Conversation Management (`core/src/context_manager/`)

**`ContextManager`** (history.rs ~line 24):
```rust
struct ContextManager {
    items: Vec<ResponseItem>,    // oldest first
    token_info: Option<TokenUsageInfo>,
}
```

Key operations:
- `record_items()` — append API/ghost snapshot items
- `for_prompt()` — prepare items for model, strip images if over budget
- `remove_first_item()` — drop oldest + its tool-result pair
- `drop_last_n_user_turns()` — rollback support

**Three compaction strategies**:

| Strategy | When | How |
|----------|------|-----|
| Pre-sampling | Before each LLM call | Trim history to fit context window |
| Auto-compaction | On `ContextWindowExceeded` error mid-turn | Compact and retry same request |
| Remote compaction | If backend service configured | Calls external compaction API |

**Token estimation**: byte-count heuristic via `estimate_token_count()` (no model call needed).

**Canon analogue**: Canon has no context window management. The tlog is unbounded; the
planner prompt includes `batch_acted` which grows without limit. Long sessions will hit token
limits silently.

---

### B.6 Permission / Approval Model (`core/src/tools/sandboxing.rs`)

**Three approval types**:

| Type | Trigger | Path |
|------|---------|------|
| `ExecApproval` | Shell command needs elevated sandbox | `Op::ExecApproval` submission |
| `PatchApproval` | `apply_patch` on risky file | Auto-approve safe, user-prompt risky |
| `NetworkApproval` | Outbound network call | `NetworkApprovalContext` created |

**`AskForApproval` policy** (protocol.rs): session-level config controlling when user
prompts are required. Equivalent to Claude Code's permission mode but per-operation-type.

**Seatbelt sandbox** (macOS, `seatbelt.rs` ~line 34):
- Uses `sandbox-exec` with dynamically generated `.sbpl` policies
- Policy files: `seatbelt_base_policy.sbpl`, `seatbelt_network_policy.sbpl`
- Network proxy policy generated from config at runtime

**ExecPolicy rules engine** (`execpolicy` crate):
- Declarative rules loaded from config file
- `ExecPolicyManager::load()` at startup
- Validates commands before execution (not a hook — runs inside `ToolRegistry`)

**Canon analogue**: No sandbox, no approval workflow, no exec policy engine. The only
protection is the planner prompt's policy text: "do not emit destructive commands".

---

### B.7 Sub-Agent System (`core/src/agent/`, `tools/handlers/multi_agents.rs`)

**5 sub-agent control tools** (exact API):

```
spawn_agent(config)       → agent_id
send_input(agent_id, msg) → ack
resume_agent(agent_id)    → ack
wait(agent_id)            → final result
close_agent(agent_id)     → ack
```

**`AgentControl`** (agent/control.rs ~line 16):
- Central control plane shared across all agents in a session
- Each sub-agent runs in its own OS thread with its own `Codex` instance
- Status broadcast via `tokio::sync::watch::Receiver<AgentStatus>`

**Agent status states**: `PendingInit`, `Running`, `WaitingForInput`, `Complete`, `Failed`, `NotFound`

**Depth limiting** (agent/guards.rs):
- `MAX_THREAD_SPAWN_DEPTH` — prevents infinite nesting
- `exceeds_thread_spawn_depth_limit()` — checked at spawn time
- If exceeded: collaboration tools disabled for that sub-agent

**Key difference from Claude Code agents**: codex sub-agents are full `Codex` instances
(same tool set, full history, approval model) running in separate threads. Claude Code's
`Agent` tool runs a stripped-down `gu1()` call with limited scope.

**Canon analogue**: No sub-agent mechanism. The single `PlanConsumer` → `ActConsumer`
pipeline has no way to delegate work to a parallel agent.

---

### B.8 Plan Mode (`core/src/codex.rs` ~line 5052, `collaboration_mode.rs`)

**Two collaboration modes**:
- `ModeKind::Default` — normal tool execution
- `ModeKind::Plan` — model proposes plan only, no tool execution

**Streaming plan detection** (`proposed_plan_parser.rs`):
- Detects `<proposed_plan>` XML tags in streamed output
- Buffers plan content separately from regular assistant text
- Defers emitting assistant messages until non-plan text detected
- Flushes buffered plan on `OutputItemDone`

**Plan mode state tracking**:
```rust
struct PlanModeStreamState {
    plan_parsers: PlanParsers,
    pending_agent_message_items: HashMap<String, TurnItem>,
    ...
}
struct ProposedPlanItemState {
    item_id: String,
    started: bool,
    completed: bool,
}
```

**Canon analogue**: Canon has no plan mode. The planner emits `LoopPlanned` events which
are immediately queued for execution — there is no "show the plan and wait for approval"
step before acting.

---

### B.9 Unique Patterns Not in Claude Code

#### Turn Diff Tracker (`tools/context.rs`, `turn_diff_tracker.rs`)
Tracks which files changed during a turn. Created per task, persisted across turns.
Used for incremental patch generation — the model can reference "what I changed so far"
without re-reading files. **Canon equivalent**: none; the planner has no file-change awareness.

#### Ghost Snapshots (`tasks.rs`, `GhostSnapshotTask`)
Background snapshot of session state while agent is running. Restored if connection drops.
Allows crash recovery without replaying the full history. **Canon equivalent**: the cursor
file tracks tlog position but not in-memory session state.

#### Skills System (`skills/` crate)
Loadable `.md` skill files injected into the prompt before each turn. Skills can declare
dependencies on MCP servers. Missing MCP server triggers user prompt to install.
**Canon equivalent**: none; the AGENT_GOAL.md serves a similar static role but isn't
dynamically loadable or composable.

#### Tool Gate Readiness Flag (`tools/parallel.rs` ~line 81)
Mutating tools (write, patch, shell) block on `tool_call_gate.wait_ready()` before execution.
The gate is opened by the approval system. This means reads can proceed in parallel while
waiting for human approval of a write. **Canon equivalent**: none; all actions are serial.

#### Dynamic Tool Registration (`tools/handlers/dynamic.rs`)
Tools can be registered at runtime via `DynamicToolSpec`. No code deployment needed to add
a tool — the model receives the schema and the handler is wired at session start.
**Canon equivalent**: the capability registry supports runtime registration but
there is no protocol for an external caller to inject tools per-session.

#### `request_user_input` Tool
Mid-turn tool that pauses execution and requests a human response. The agent can ask a
clarifying question, receive the answer as a tool result, and continue the same turn.
**Canon equivalent**: none; the routing gate selects `scan` to wait for user input but
this is a full turn boundary, not a mid-turn pause.

#### BM25 File Search (`file-search` crate, `search_tool_bm25`)
Built-in semantic search over workspace files. The model can search by content query
rather than having to know file names. **Canon equivalent**: none; the LLM must specify
exact file paths in `write_file`/`patch_file` actions.

---

### B.10 Comparison Table: Claude Code vs codex-rs vs Canon

| Feature             | Claude Code (`tengu`)           | codex-rs                            | Canon                        |
|---------------------+---------------------------------+-------------------------------------+------------------------------|
| Agent loop          | Recursive `yield*` generator    | `while needs_follow_up` loop        | Event-driven tick (P3 timer) |
| Tool parallelism    | Sequential per turn             | Parallel (RwLock read/write)        | Sequential (one action/tick) |
| Pre-tool hook       | `PreToolUse` (allow/deny/ask)   | Tool gate (approval)                | None                         |
| Post-tool hook      | `PostToolUse` (suppress/inject) | `AfterToolUse` hook                 | `LoopVerified` event         |
| Turn-end hook       | `Stop` / `SubagentStop`         | `AfterAgent` hook                   | `LoopRewarded` event         |
| Context mgmt        | Auto-compact (LLM summarise)    | Pre-sampling + auto-compact         | None (unbounded tlog)        |
| Sub-agents          | Single-level (stripped)         | Hierarchical (full Codex instances) | None                         |
| Plan mode           | Via `plan` permission mode      | `ModeKind::Plan` + XML tags         | None                         |
| Sandbox             | macOS seatbelt / network proxy  | Linux seccomp / macOS seatbelt      | None                         |
| File search         | Via Bash/Glob tools             | Built-in BM25                       | None                         |
| Skill injection     | Plugin skills via CLAUDE.md     | `.md` skill files, MCP-aware        | Static `AGENT_GOAL.md`       |
| Mid-turn user input | `Notification` hook             | `request_user_input` tool           | None (full turn boundary)    |
| Crash recovery      | JSONL session files             | Ghost snapshots                     | Cursor file (tlog offset)    |
| Dynamic tools       | MCP tools                       | Dynamic tool registration           | Capability registry          |


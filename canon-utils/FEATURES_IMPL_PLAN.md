# Canon Features Implementation Plan

Implementation plans for the codex agent to execute.
Each plan is self-contained with file targets, exact changes, and acceptance criteria.

---

## PLAN-1: Rich Error Feed + Delta Error Tracking

**Feature**: Show up to 5 structured compiler errors with file:line context in the planner
prompt. Track new vs. pre-existing errors.

**Files to change**:
- `canon-utils/canon-observe/src/lib.rs`
- `canon-utils/canon-plan/src/lib.rs`
- `canon-utils/canon-runtime-events/src/events.rs` (extend `LoopObserved`)

**Changes**:

1. `LoopObserved` — add fields:
   ```rust
   pub new_error_count: u32,
   pub compiler_errors: Vec<CompilerError>,
   ```
   where `CompilerError` is:
   ```rust
   canon_event_struct!(CompilerError {
       file: String,
       line: u32,
       col: u32,
       message: String,
       context_lines: Vec<String>,
   });
   ```

2. `ObserveConsumer` (`canon-observe/src/lib.rs`) — compare current `compiler_errors`
   against a stored `prev_errors: Vec<CompilerError>`. Set `new_error_count` = count of
   errors whose `(file, line, message)` triple was not in `prev_errors`. Store current errors
   as `prev_errors` for next observe cycle.

3. `build_prompt` in `canon-plan/src/lib.rs` — replace the existing `## Compiler Errors (N)`
   section (which shows only the first error) with:
   ```
   ## Compiler Errors (N total, K new since last action)
   ### Error 1
   file: src/main.rs:42:8
   message: expected `}`, found `fn`
   context:
     40 |   fn foo() {
     41 |       let x = 1
     42 |   fn bar() {
   ```
   Read context lines from disk using `std::fs::read_to_string`. Cap at 5 errors.
   Each error gets 3 lines of context centred on the error line.

**Acceptance criteria**:
- Planner prompt shows up to 5 errors with file:line reference.
- `new_error_count` in `LoopObserved` is 0 when no new errors since last observe.
- Existing `error_count` field preserved for backward compat.

---

## PLAN-2: Pre-Execution Destructive Command Block

**Feature**: `ActConsumer` checks shell commands against a deny/warn list before dispatching.

**Files to change**:
- `canon-utils/canon-act/src/lib.rs` (or the file containing `ActConsumer`)

**Changes**:

1. Add constant deny list (exact match on prefix after shell-splitting):
   ```rust
   const BLOCKED_PATTERNS: &[&str] = &[
       "rm -rf", "git reset --hard", "git clean -f",
       "dd if=", "mkfs", "shred", ">/dev/sd",
   ];
   const WARN_PATTERNS: &[&str] = &[
       "git push --force", "cargo clean", "truncate",
   ];
   ```

2. Before dispatching `run_command` capability: check `cmd` field (the full shell command
   string) using `BLOCKED_PATTERNS.iter().any(|p| cmd.contains(p))`.

3. If blocked:
   - Emit `LoopActed { success: false, stderr: "blocked:destructive_command",
     exit_code: Some(-1), ... }` **without** calling the executor.
   - Log a `Debug` event: `kind = "pre_exec_blocked"`, payload `{ "cmd": cmd, "pattern": matched_pattern }`.

4. If warned (not blocked):
   - Emit `Debug` event: `kind = "pre_exec_warning"`, payload `{ "cmd": cmd, "pattern": matched_pattern }`.
   - Proceed with execution.

**Acceptance criteria**:
- `rm -rf /` in a shell action emits `LoopActed { success: false, stderr: "blocked:destructive_command" }`.
- `cargo clean` emits a `Debug` warn event and still executes.
- `cargo build` is unaffected.

---

## PLAN-3: Repeated-Failure Deduplication Guard

**Feature**: Prevent the planner from dispatching identical LLM calls after a repeated
failing action. Emit `no_op` if the proposed plan is identical to the last failed one.

**Files to change**:
- `canon-utils/canon-plan/src/lib.rs`

**Changes**:

1. Add fields to `PlanConsumer` (or equivalent plan state struct):
   ```rust
   last_planned_action_signature: Option<String>,
   last_plan_succeeded: bool,
   ```

2. `action_signature(kind: &str, cmd: &str) -> String` — `format!("{kind}:{cmd}")`.

3. After a `LoopActed { success: false }` is received in the plan consumer's state update:
   store `last_plan_succeeded = false`.

4. After a `LoopActed { success: true }` — clear `last_planned_action_signature = None`,
   set `last_plan_succeeded = true`.

5. When `handle_observed` would dispatch a new LLM call: first compute the planned action's
   signature. If `!last_plan_succeeded && last_planned_action_signature == Some(&new_sig)`:
   - Skip the LLM call.
   - Emit `LoopPlanned { action_kind: "no_op", reason: "plan_identical_to_failed", ... }`.

6. Otherwise dispatch the LLM call and store the new signature in
   `last_planned_action_signature`.

**Acceptance criteria**:
- If the LLM returns `cargo new foo` twice and `cargo new foo` fails both times, the second
  invocation is suppressed and `LoopPlanned { action_kind: "no_op" }` is emitted.
- After a successful action, dedup guard resets and next LLM call is dispatched normally.

---

## PLAN-4: Replanning Context (Anti-Retry Prompt Injection)

**Feature**: When the last action failed, inject a `## Replanning Context` block into the
planner prompt instructing the LLM to take a different approach.

**Files to change**:
- `canon-utils/canon-plan/src/lib.rs` (`build_prompt`)

**Changes**:

1. `build_prompt` takes `last_failed_action: Option<(String, String)>` — (action_kind, cmd).

2. If `Some((kind, cmd))`:
   ```
   ## Replanning Context
   The previous approach failed. Generate ONE alternative action that takes a
   DIFFERENT approach than: "{kind}: {cmd}".
   Do not retry the same command with the same arguments.
   ```

3. Pass `last_failed_action` from `PlanConsumer` state — populated from `LoopActed {
   success: false }` events. Cleared on any successful `LoopActed`.

**Acceptance criteria**:
- After a failed `run_command("cargo new foo")`, the planner prompt contains the
  `## Replanning Context` block with `"run_command: cargo new foo"`.
- After a successful action the block is absent from the next prompt.

---

## PLAN-5: Confidence-Based Verify Severity Scoring

**Feature**: Add `severity_score: u8` (0–100) to `LoopVerified`. Surface it in the planner
prompt so the LLM can distinguish cosmetic warnings from hard failures.

**Files to change**:
- `canon-utils/canon-runtime-events/src/events.rs` (extend `LoopVerified`)
- `canon-utils/canon-verify/src/lib.rs` (compute the score)
- `canon-utils/canon-plan/src/lib.rs` (show in prompt)

**Changes**:

1. `LoopVerified` — add field:
   ```rust
   pub severity_score: u8,
   ```

2. `VerifyConsumer` — compute weighted sum:
   | Signal | Weight |
   |---|---|
   | compiler error | 100 (capped at 100) |
   | compiler warning | 20 |
   | tlog `error_occurred` | 60 |
   | `file_path_not_found` for write action | 80 |
   | skipped action (batch abort) | 0 |

   `severity_score = min(100, sum_of_weights)`.

3. `build_prompt` — in the `## Verification` section, add:
   ```
   severity_score: {score}/100
   ```
   When `passed = false` but `severity_score < 30`, append:
   `(minor — consider routing to validate rather than reshaping)`.

**Acceptance criteria**:
- A compile warning only produces `severity_score = 20`.
- A compiler error produces `severity_score = 100`.
- Score appears in the planner prompt.

---

## PLAN-6: Crash Recovery — Extended Cursor Context

**Feature**: Persist planning context (last failed action, shape tick count, current phase)
to the cursor file so a restarted runtime has context from the prior session.

**Files to change**:
- `canon-utils/canon-runtime/src/bin/event_runtime.rs`

**Changes**:

1. Extend cursor JSON schema with optional fields:
   ```json
   {
     "last_failed_action_kind": "run_command",
     "last_failed_cmd": "cargo new foo",
     "shape_tick_count": 7,
     "current_phase": 1
   }
   ```

2. `load_cursor_state` — read and populate `RouteRuntimeState` (or the equivalent state
   struct) with these fields on startup.

3. `save_cursor` — extend to serialise these four fields alongside the existing
   `processed`, `start_seq`, `session_id`, `next_id`.

4. `RouteRuntimeState` — add the four optional fields:
   ```rust
   pub last_failed_action_kind: Option<String>,
   pub last_failed_cmd: Option<String>,
   pub shape_tick_count: u64,
   pub current_phase: usize,
   ```

**Acceptance criteria**:
- After a session where `cargo new foo` failed 3 times, the cursor JSON contains
  `"last_failed_cmd": "cargo new foo"`.
- On restart (replaying events from the cursor position), the first `build_prompt` call
  includes the `## Replanning Context` block if the last action was a failure.

---

## PLAN-7: Context Compaction — Planner Prompt Summarisation

**Feature**: When the planner's `batch_acted` list exceeds N entries, summarise the oldest
entries with a separate LLM call and replace them with a compact summary block.

**Files to change**:
- `canon-utils/canon-plan/src/lib.rs`

**Changes**:

1. When `batch_acted.len() > 16`:
   - Take the oldest 8 entries.
   - Fire a separate `llm.call` capability with a summarisation prompt:
     ```
     Summarise these {N} completed actions in ≤3 bullet points, focusing on
     what changed and what failed:
     {entries formatted as "action_kind: cmd — success={bool}"}
     ```
   - Replace the 8 oldest entries with a single synthetic entry:
     ```rust
     BatchActed { action_kind: "summary".into(), summary_text: llm_response, ... }
     ```

2. The summary entry is rendered in `build_prompt` as:
   ```
   [summary of 8 earlier actions]:
   - Scaffolded Cargo.toml and src/main.rs
   - cargo new failed 3 times (wrong dir)
   - Fixed by using cargo init instead
   ```

3. A `CompactionTriggered` debug event is emitted when compaction fires.

**Acceptance criteria**:
- After 17 actions, the batch_acted list stays at ≤ 9 entries (8 compacted to 1 + 9 new).
- The summary text appears verbatim in the next planner prompt.

---

## PLAN-8: BM25 File Search Integration

**Feature**: Add a `search_files` action type backed by BM25 index so the planner can query
relevant files before making write decisions.

**Files to change**:
- New crate: `canon-utils/canon-search/` (Cargo.toml + src/lib.rs)
- `canon-utils/canon-plan/src/lib.rs` (handle `search_files` action kind)
- `canon-utils/canon-act/src/lib.rs` (dispatch search capability)
- `Cargo.toml` (workspace member)

**Changes**:

1. `canon-search` crate — wraps `bm25` crate (or implement manually):
   - `BM25Index::build(workspace: &Path) -> Self` — walks `.rs`, `.toml`, `.md` files,
     tokenises by whitespace + punctuation, builds inverted index.
   - `BM25Index::query(q: &str, top_k: usize) -> Vec<SearchResult>` where
     `SearchResult = { path: PathBuf, score: f32, snippet: String }`.
   - Index is rebuilt on each `LoopVerified` event (workspace may have changed).

2. `LlmAction` — add new variant:
   ```rust
   SearchFiles { query: String, top_k: usize },
   ```

3. `ActConsumer` — for `SearchFiles`:
   - Call `BM25Index::query`.
   - Emit `LoopActed { action_kind: "search_files", stdout: json results, success: true }`.

4. Planner prompt — include top-3 search results in `## Workspace State` when
   `last_search_results` is non-empty:
   ```
   ## Relevant Files (BM25)
   [0.92] src/handlers/user.rs — "fn handle_user_request"
   [0.87] src/lib.rs — "pub mod handlers"
   ```

**Acceptance criteria**:
- `SearchFiles { query: "handle user request" }` returns the top 3 relevant `.rs` files.
- Results appear in the next planner prompt under `## Relevant Files`.
- Index rebuild does not block the main event loop (run on a separate thread or via
  `spawn_blocking`).

---

## PLAN-9: Phase Tracking in Goal Spec and LoopObserved

**Feature**: Parse `## Phases` from `AGENT_GOAL.md`. Evaluate which phase criteria are
satisfied. Emit `current_phase` in `LoopObserved` and show it in the planner prompt.

**Files to change**:
- `canon-utils/canon-goal/src/lib.rs`
- `canon-utils/canon-observe/src/lib.rs`
- `canon-utils/canon-runtime-events/src/events.rs` (extend `LoopObserved`)
- `canon-utils/canon-plan/src/lib.rs`

**Changes**:

1. `GoalSpec` — add field:
   ```rust
   pub phases: Vec<GoalPhase>,
   ```
   where `GoalPhase = { name: String, criteria: Vec<String> }`.

2. `parse_agent_goal_markdown` — parse a `## Phases` section formatted as:
   ```markdown
   ## Phases
   1. Scaffold: Cargo.toml and src/main.rs exist
   2. Core: All required functions implemented
   3. Polish: README.md non-empty, cargo build clean
   ```
   Each numbered item: `name = "Scaffold"`, `criteria = ["Cargo.toml and src/main.rs exist"]`.

3. `ObserveConsumer` — after building `GoalSpec`, evaluate phase criteria against the
   workspace (file existence checks, `compiler_clean` flag). Determine `current_phase: usize`
   = highest phase index where all criteria pass.

4. `LoopObserved` — add field:
   ```rust
   pub current_phase: Option<usize>,
   pub phase_name: Option<String>,
   ```

5. `build_prompt` — in `## Active Goal`:
   ```
   Current phase: 1 (Scaffold) — completed
   Next phase: 2 (Core) — in progress
   ```

**Acceptance criteria**:
- A goal markdown with 3 phases emits `current_phase = 1` after only phase 1 criteria pass.
- Planner prompt includes the phase progress line.
- Goals without a `## Phases` section emit `current_phase = None` (no regression).

---

## PLAN-10: Tool Gate / Approval for Mutating Operations

**Feature**: Before dispatching mutating tool calls (writes, shell commands), wait for an
approval token. Supports both auto-approve and interactive modes.

**Files to change**:
- `canon-utils/canon-act/src/lib.rs`
- `canon-utils/canon-runtime-events/src/events.rs` (new `ToolApprovalRequired` event)
- New: `canon-utils/canon-gate/src/lib.rs` (gate state machine)

**Changes**:

1. Two new events:
   ```rust
   canon_event_struct!(ToolApprovalRequired {
       request_id: String,
       action_kind: String,
       payload: serde_json::Value,
   });
   canon_event_struct!(ToolApprovalGranted {
       request_id: String,
       approved: bool,
   });
   ```

2. `GatePolicy` enum:
   ```rust
   pub enum GatePolicy {
       AutoApprove,        // always approve (default, current behaviour)
       BlockDestructive,   // auto-approve safe; require approval for destructive
       RequireAll,         // require approval for all mutating actions
   }
   ```

3. `ActConsumer` — before dispatching, check action kind against `GatePolicy`:
   - If policy allows → proceed immediately.
   - If policy requires approval → emit `ToolApprovalRequired` and suspend the pending
     action in `pending_approvals: HashMap<String, PendingAction>`.
   - On `ToolApprovalGranted { approved: true }` → dispatch the stored action.
   - On `ToolApprovalGranted { approved: false }` → emit `LoopActed { success: false,
     stderr: "blocked:user_denied" }`.

4. `GatePolicy` is read from `AGENT_GOAL.md` frontmatter or a runtime config field.

**Acceptance criteria**:
- With `GatePolicy::BlockDestructive`, a `run_command("rm -rf ...")` pauses and emits
  `ToolApprovalRequired`.
- With `GatePolicy::AutoApprove`, all actions proceed without pausing (no regression).
- `ToolApprovalGranted { approved: false }` results in `LoopActed { success: false }`.

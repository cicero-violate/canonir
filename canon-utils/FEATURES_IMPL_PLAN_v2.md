# Canon Features Implementation Plan v2

> Generated 2026-03-23. Based on audit of current codebase against FEATURES_IMPL_PLAN.md.
> The original plan was written against an older codebase — file paths and struct names
> have been updated to reflect actual locations.

---

## Status Summary

| Plan | Title | Status | Notes |
|------|-------|--------|-------|
| PLAN-1 | Rich Error Feed + Delta Error Tracking | PARTIAL | `compiler_errors` exists, `new_error_count` / structured type missing |
| PLAN-2 | Pre-Execution Destructive Command Block | IMPLEMENTED | `is_potentially_destructive` in act.rs:887 |
| PLAN-3 | Repeated-Failure Deduplication Guard | NOT IMPLEMENTED | — |
| PLAN-4 | Replanning Context (Anti-Retry Injection) | NOT IMPLEMENTED | — |
| PLAN-5 | Confidence-Based Verify Severity Scoring | NOT IMPLEMENTED | — |
| PLAN-6 | Crash Recovery — Extended Cursor Context | NOT IMPLEMENTED | — |
| PLAN-7 | Context Compaction — Planner Prompt Summarisation | NOT IMPLEMENTED | — |
| PLAN-8 | BM25 File Search Integration | PARTIAL | BM25 implemented, not exposed as LLM action |
| PLAN-9 | Phase Tracking in Goal Spec | NOT IMPLEMENTED | — |
| PLAN-10 | Tool Gate / Approval for Mutating Operations | NOT IMPLEMENTED | — |

---

## PLAN-1: Rich Error Feed + Delta Error Tracking — PARTIAL

**What exists:**
- `LoopObserved.compiler_errors: Vec<serde_json::Value>` at `canon-runtime-events/src/events.rs:142`
- `LoopContext.recent_compiler_errors` populated in `canon-loop/src/executor.rs` (capped at 16)
- Errors forwarded to `LoopObserved` in `canon-loop/src/stage/observe.rs:32`
- Planner prompt shows `Errors: N` count in the header line

**What's missing:**
- No structured `CompilerError { file, line, col, message, context_lines }` type — using raw JSON
- No `new_error_count` field on `LoopObserved` (delta tracking)
- No `prev_errors` comparison in observe stage
- Planner prompt shows error count only, not file:line context or code snippet

**Pending work:**

1. Add structured type to `canon-runtime-events/src/events.rs`:
   ```rust
   canon_event_struct!(CompilerError {
       file: String,
       line: u32,
       col: u32,
       message: String,
       context_lines: Vec<String>,
   });
   ```
   Change `compiler_errors: Vec<serde_json::Value>` → `compiler_errors: Vec<CompilerError>`.

2. Add `new_error_count: u32` to `LoopObserved`.

3. In `canon-loop/src/stage/observe.rs` — parse `cargo check` stderr into `CompilerError`,
   store prev cycle's errors, compute `new_error_count` as new `(file,line,message)` triples.

4. In `canon-loop/src/stage/plan.rs` `build_prompt` — replace the count-only line with a
   structured block showing up to 5 errors with 3 lines of file context each.

---

## PLAN-2: Pre-Execution Destructive Command Block — IMPLEMENTED

**What exists:**
- `is_potentially_destructive(cmd)` at `canon-loop/src/stage/act.rs:887` — checks `rm -rf`,
  `rm -fr`, `rm -r`, `rm -f`, `git reset --hard`, `git clean -f`, `dd`, `mkfs`, `shred`
- `DestructiveCmdPolicy` enum (Block/Warn/Allow) with Block as default
- Blocked commands emit `LoopActed { success: false, stderr: "rejected_destructive_command" }`
- `build_prompt` in plan.rs:429 detects the rejection and injects a warning into the next prompt
- Planner prompt `SAFETY RULE` section lists blocked patterns

**Divergence from original plan:**
- Stderr tag is `"rejected_destructive_command"` not `"blocked:destructive_command"`
- No separate `Debug` event emitted for blocked/warned commands (was removed in cleanup)
- `>/dev/sd` and `shred` patterns from original plan may not be present — verify coverage

No pending work for core functionality. Optional: add `>/dev/sd` and `shred` to the pattern
list at act.rs:887 if not already present.

---

## PLAN-3: Repeated-Failure Deduplication Guard — NOT IMPLEMENTED

**What's missing:** Everything.

**Pending work:**

1. Add to `LoopContext` in `canon-loop/src/context.rs`:
   ```rust
   pub last_plan_signature: Option<String>,
   pub last_plan_succeeded: bool,
   ```

2. Update from `LoopActed` events:
   - `success: false` → `last_plan_succeeded = false`
   - `success: true` → `last_plan_succeeded = true; last_plan_signature = None`

3. In `canon-loop/src/stage/plan.rs` `handle_observed`, after parsing LLM response:
   compute `sig = format!("{action_kind}:{cmd}")` for the primary action.
   If `!ctx.last_plan_succeeded && ctx.last_plan_signature == Some(&sig)`:
   - skip dispatch, emit `LoopPlanned { action_kind: "no_op", reason: "plan_identical_to_failed" }`
   Otherwise store new signature.

**Priority:** Medium — prevents tight retry loops on persistent failures.

---

## PLAN-4: Replanning Context (Anti-Retry Prompt Injection) — NOT IMPLEMENTED

**What's missing:** The `## Replanning Context` block in the planner prompt.

**Pending work:**

1. Add to `LoopContext`:
   ```rust
   pub last_failed_action: Option<(String, String)>, // (action_kind, cmd)
   ```
   Populate from `LoopActed { success: false }`, clear on `LoopActed { success: true }`.

2. Pass to `build_prompt` in `canon-loop/src/stage/plan.rs`. When `Some((kind, cmd))`:
   ```
   ## Replanning Context
   The previous approach failed. Generate ONE alternative action that takes a
   DIFFERENT approach than: "{kind}: {cmd}".
   Do not retry the same command with the same arguments.
   ```

**Priority:** High — directly reduces wasted cycles (seen in tlog where LLM retried identical commands).
Works well with PLAN-3 as a layered defence.

---

## PLAN-5: Confidence-Based Verify Severity Scoring — NOT IMPLEMENTED

**What's missing:** `severity_score` field and scoring logic.

**Pending work:**

1. Add `pub severity_score: u8` to `LoopVerified` in `canon-runtime-events/src/events.rs`.

2. In `canon-loop/src/stage/verify.rs`, compute:
   ```
   score = min(100, compiler_errors * 100 + warnings * 20 + tlog_errors * 60)
   ```

3. In `canon-loop/src/stage/plan.rs` `build_prompt`, add to the context:
   ```
   severity_score: {N}/100
   ```
   Append `(minor — consider validate rather than replan)` when `passed=false && score < 30`.

**Priority:** Low — cosmetic improvement to router/planner context. Not blocking.

---

## PLAN-6: Crash Recovery — Extended Cursor Context — NOT IMPLEMENTED

**What's missing:** Extended cursor persistence.

**Actual cursor location:** `canon-runtime/src/lib.rs` (cursor read/write logic). The original
plan referenced `canon-runtime/src/bin/event_runtime.rs` which no longer exists as a separate file.

**Pending work:**

1. Locate cursor save/load in `canon-runtime/src/lib.rs`. Extend cursor JSON with:
   ```json
   {
     "last_failed_action_kind": "run_command",
     "last_failed_cmd": "cargo new foo",
     "stagnant_ticks": 3
   }
   ```

2. On restart, restore these into `LoopContext` so the first `build_prompt` call includes
   `## Replanning Context` if the prior session ended on a failure.

**Dependency:** Requires PLAN-4 to be implemented first (replanning context prompt block).
**Priority:** Low — useful for long-running sessions, not critical for correctness.

---

## PLAN-7: Context Compaction — Planner Prompt Summarisation — NOT IMPLEMENTED

**What's missing:** Compaction logic and summarisation LLM call.

**Actual state:** `batch_acted` in `canon-loop/src/context.rs` is cleared after each plan
dispatch — no rolling accumulation. The original plan assumed a growing list. The architecture
has changed.

**Revised pending work:**

Instead of compacting a rolling list, the relevant mechanism is: when `batch_acted.len() > 12`
*within a single planning cycle* (many actions in one batch), summarise the oldest entries before
building the prompt.

1. In `canon-loop/src/stage/plan.rs` `handle_observed`, before calling `build_prompt`:
   if `ctx.batch_acted.len() > 12`, fire a synchronous summarisation LLM call on the oldest
   entries and replace them with a single synthetic acted entry.

2. Emit a `Debug` event `kind = "CompactionTriggered"` when this fires.

**Priority:** Low — relevant only for very large batches. Not observed in current runs.

---

## PLAN-8: BM25 File Search Integration — PARTIAL

**What exists:**
- `canon-tools-search/src/lib.rs` — full BM25 implementation (`search_files_bm25`) and nucleo
  fuzzy fallback (`search_files`)
- Called internally from `canon-loop/src/stage/plan.rs:5` via `build_search_hints`
- Results appear in the planner prompt under `Relevant files:` section

**What's missing:**
- No `search_files` action the LLM can explicitly invoke — it's planner-internal only
- No per-LoopVerified index rebuild (currently called on every prompt build)
- No `LlmAction::SearchFiles` variant in the plan parser

**Pending work:**

1. Add `SearchFiles` to `LlmAction` enum in `canon-loop/src/stage/plan.rs`:
   ```rust
   SearchFiles { query: String },
   ```

2. Dispatch in `canon-loop/src/stage/act.rs`: call `canon_tools_search::search_files_bm25`,
   emit `LoopActed { action_kind: "search_files", stdout: json_results }`.

3. Add to planner prompt tool list:
   ```
   3. search_files — find relevant files by keyword query
      {"action":"search_files","query":"handle user request"}
   ```

**Priority:** Medium — gives the LLM explicit control over file discovery instead of relying
on planner-internal pre-search.

---

## PLAN-9: Phase Tracking in Goal Spec — NOT IMPLEMENTED

**What's missing:** Everything. `GoalSpec` has no `phases` field; `LoopObserved` has no
`current_phase`.

**Pending work:**

1. In `canon-goal/src/lib.rs`, add:
   ```rust
   pub struct GoalPhase { pub name: String, pub criteria: Vec<String> }
   ```
   Add `pub phases: Vec<GoalPhase>` to `GoalSpec`.
   Parse `## Phases` numbered list in `parse_agent_goal_markdown`.

2. Add `pub current_phase: Option<usize>` and `pub phase_name: Option<String>` to
   `LoopObserved` in `canon-runtime-events/src/events.rs`.

3. In `canon-loop/src/stage/observe.rs`, evaluate phase criteria (file existence, compile clean)
   and emit `current_phase`.

4. In `canon-loop/src/stage/plan.rs` `build_prompt`, render phase progress.

**Priority:** Low — nice for structured multi-phase goals, not needed for simple tasks.

---

## PLAN-10: Tool Gate / Approval for Mutating Operations — NOT IMPLEMENTED

**What's missing:** Everything. No `ToolApprovalRequired`, no `GatePolicy`, no approval queue.

**Pending work:**

1. Add two events to `canon-runtime-events/src/events.rs`:
   ```rust
   canon_event_struct!(ToolApprovalRequired { request_id: String, action_kind: String, payload: serde_json::Value });
   canon_event_struct!(ToolApprovalGranted  { request_id: String, approved: bool });
   ```

2. Add `GatePolicy` enum and read from goal frontmatter or env var.

3. In `canon-loop/src/stage/act.rs`, gate mutating actions behind policy check.

**Priority:** Low — needed for interactive/supervised mode. Current use is fully autonomous.
Implement after PLAN-4 and PLAN-3 since those reduce the cases where harmful retries happen.

---

## Recommended Implementation Order

Based on impact observed in live tlogs:

1. **PLAN-4** — Replanning context injection. Directly reduces retry loops seen in production runs.
2. **PLAN-3** — Dedup guard. Pairs with PLAN-4 as a hard stop for identical-failure loops.
3. **PLAN-1** (complete) — Structured error feed. Improves planner's ability to fix compile errors.
4. **PLAN-8** (complete) — Expose `search_files` as LLM action. Low effort, already built.
5. **PLAN-5** — Severity scoring. Improves router signal quality.
6. **PLAN-6** — Cursor recovery. Requires PLAN-4 as prerequisite.
7. **PLAN-9** — Phase tracking. Useful for complex multi-phase goals.
8. **PLAN-7** — Compaction. Only needed at scale.
9. **PLAN-10** — Approval gate. Supervised mode, low urgency.

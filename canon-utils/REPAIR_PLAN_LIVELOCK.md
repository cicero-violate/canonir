# REPAIR_PLAN_LIVELOCK.md

**Source:** Log analysis — run 1 (126 events, ticks 1–51) + run 2 (151 events, ticks 1–58) + run 3 (131 events, ticks 1–78).
**Core problems:**
1. ~~`(tick → observe)^n`~~ — **FIXED** in run 3 (workspace_facts field added, dedup working)
2. ~~Planner response format~~ — **FIXED** in run 3 (returns flat JSON array)
3. Mid-batch deadlock: `execute_complete` does not re-enter the dispatch loop → still pending
4. **NEW (run 3):** False completion — `list_dir` triggers verify; pre-existing project passes; loop halts without doing any real work
5. **NEW (run 3):** No hard gate rule for `finish_ready → conclude`; takes an extra LLM round-trip

---

## Issue inventory

| ID | Status | Severity | Component | Description |
|----|--------|----------|-----------|-------------|
| LIVE-1 | ✅ FIXED | Critical | canon-loop / stage/observe.rs | Observe dedup implemented — only 1 LoopObserved before first RouteSelected |
| PLAN-FORMAT-1 | ✅ FIXED | High | canon-exec / plan prompt | Planner now returns flat JSON array — `parse_ok=true` |
| LIVE-3 | ⏳ Pending | **Critical** | canon-loop / stage/act.rs | `execute_complete` stops after first async action — remaining batch actions never dispatched |
| VERIFY-1 | ⏳ Pending | **Critical** | canon-route / context.rs | `list_dir` sets `workspace_dirty=true` + `acted_unverified=true` → forces verify on read-only action |
| VERIFY-2 | ⏳ Pending | **Critical** | canon-loop / stage/verify.rs | `evaluate_goal_satisfied` returns true for pre-existing project → false completion |
| GATE-1 | ⏳ Pending | High | canon-judgment / src/lib.rs | No hard gate rule for `finish_ready=true → conclude`; gate accepts wrong routes; needs extra LLM round-trip |
| LIVE-2 | ⏳ Pending | High | canon-loop / stage/mod.rs | `RouteSelected=observe` (Scan arm) fires a second observe on same tick |
| PLAN-CONTEXT-1 | ⏳ Pending | High | canon-loop / stage/observe.rs | `workspace_facts: []` exists but is always empty — planner sees no filesystem state |
| PLAN-PREMATURE-DONE | ⏳ Pending | Medium | canon-loop / stage/plan.rs | `done` alongside other actions in same batch — requires code-level strip guard |
| PLAN-REPLAN-GUARD | ⏳ Pending | **Critical** | canon-loop / src/executor.rs | `last_planned_observed_tick` only cleared on failure; after successful batch plan stage returns Noop forever → plan→Noop infinite loop |
| PLAN-WORKSPACE-RULE | ⏳ Pending | High | canon-loop / stage/plan.rs (prompt) | WORKSPACE RULE says "dir exists → cargo init" but cargo init also fails when Cargo.toml exists; no handling for existing-project case |

---

## REPAIR-LIVE-3 — Fix mid-batch deadlock: `execute_complete` must continue dispatch loop

**File:** `canon-utils/canon-loop/src/stage/act.rs`

**Root cause (confirmed by source):**
`execute_complete` (line 50–70) takes `ctx.pending_act`, emits ToolResult + LoopActed for the
completed action, then returns. It never re-enters the dispatch loop.

`execute_dispatch` (line 16–48) contains a `while ctx.pending_act.is_none()` loop that
dispatches the entire same-batch queue sequentially — halting when it sets `pending_act` for
the first async action (e.g., `run_command`). When that async action completes, `execute_complete`
is called but does NOT resume the loop. The remaining queue (`apply_patch`, `done`) stays in
`ctx.act_queue` forever, unreachable because:
- `RouteTick` idle guard requires `planned_pending == 0` (not met: still 2)
- `batch_settled` requires `planned_pending == 0` (not met)
- No other event path triggers `execute_dispatch`

**Evidence (run 2):**
- Event 115: `cargo build` CapabilityCompleted (success)
- Event 118: LoopActed run_command — `planned_pending` drops to 2
- Events 119–151: only LoopObserved events — `apply_patch` and `done` never dispatched
- Log ends at tick 58 with `apply_patch` and `done` still queued

**Fix:** After emitting LoopActed in `execute_complete`, continue dispatching the remaining
same-batch actions using the same loop logic as `execute_dispatch`:

```rust
pub fn execute_complete(c: CapabilityCompleted, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let Some(pending) = ctx.pending_act.take() else {
        return Ok(LoopStageResult::Noop);
    };
    if pending.request_id != c.request_id {
        ctx.pending_act = Some(pending);
        return Ok(LoopStageResult::Noop);
    }
    let (stdout, stderr, exit_code, duration_ms, success) = extract_result_fields(&c.result, pending.started_at);
    let action_kind = pending.action_kind.clone();
    let llm_request_id = pending.llm_request_id.clone();
    let tool_result_id = Uuid::new_v4().to_string();
    let mut events = Vec::new();
    events.push(emit_tool_result(ctx, &pending, tool_result_id.clone(), c.result.clone(), success));
    ctx.mark_batch_completion(llm_request_id.as_deref(), success);
    events.push(emit_acted(pending, stdout, stderr, exit_code, duration_ms, success, Some(tool_result_id)));
    if !success && action_kind == "run_command" {
        events.extend(abort_active_batch(ctx));
    } else {
        // ← NEW: continue same-batch dispatch after async action completes
        while ctx.pending_act.is_none() {
            let Some(next) = ctx.act_queue.front() else { break; };
            if next.llm_request_id != llm_request_id { break; }
            let next = ctx.act_queue.pop_front().expect("front exists");
            match dispatch_plan(ctx, &next)? {
                LoopStageResult::Emit(e) => events.push(e),
                LoopStageResult::EmitMany(evs) => events.extend(evs),
                _ => {}
            }
        }
    }
    Ok(LoopStageResult::EmitMany(events))
}
```

**Effect:** After each async action completes, the next action in the same batch is dispatched
immediately. No extra LLM roundtrip needed. If the next action is also async, `pending_act` is
set again and the loop halts until that one completes. Synchronous actions (like `list_dir`) are
drained immediately.

---

## REPAIR-LIVE-1 — Suppress observe when router LLM is in-flight and state is unchanged

**File:** `canon-utils/canon-loop/src/context.rs` + `canon-utils/canon-loop/src/stage/observe.rs`

**Root cause:**
`stage/mod.rs` line 63: `RuntimeEvent::Tick(t) => Ok(LoopStageEvent::Observe(t))`
Every `Tick` (emitted every second by `emit_tick()`) unconditionally calls `observe::execute`.
The router LLM call takes ~20 seconds → 20 identical `LoopObserved` events with no state change.

**Evidence:** Events 4, 6, 8, 9, 14, 16, 18, 19, … 94 — 22 `LoopObserved` events before first `RouteSelected`.

**Fix:** Add a `last_observed_error_count` + `last_observed_goal_hash` pair to `LoopContext`.
In `observe::execute`, return `Noop` if nothing has changed since last observe:

```rust
// In LoopContext (context.rs):
pub last_observed_error_count: u64,
pub last_observed_goal_hash: u64,  // hash of goal_text

// In observe::execute (observe.rs):
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

let goal_hash = {
    let mut h = DefaultHasher::new();
    ctx.goal_text.hash(&mut h);
    h.finish()
};
let state_changed = ctx.error_count != ctx.last_observed_error_count
    || goal_hash != ctx.last_observed_goal_hash;

if !state_changed {
    return Ok(LoopStageResult::Noop);
}
ctx.last_observed_error_count = ctx.error_count;
ctx.last_observed_goal_hash = goal_hash;
// ... proceed to emit LoopObserved
```

**Effect:** Reduces observe spam from ~1/sec to only when error_count or goal_text changes.
First observe still fires (initial state differs from zero values). Subsequent ticks are suppressed until state evolves.

---

## REPAIR-LIVE-2 — Remove double-observe on RouteSelected=observe (Scan arm)

**File:** `canon-utils/canon-loop/src/stage/mod.rs`

**Root cause:**
`Scan(rs)` arm (line 27–31) calls `observe::execute` again when `RouteSelected=observe`.
Combined with `Tick → Observe`, this means every "observe" route selection produces
two consecutive `LoopObserved` events (one from the prior Tick, one from the Scan arm).

**Evidence:** Events 8 + 9 both fire for the same RouteTick — ticks 3 and 4 in one cycle.

**Fix:** With REPAIR-LIVE-1 applied, the deduplication guard in `observe::execute` will suppress
the second identical observe automatically. No code change needed in `mod.rs` — LIVE-1 covers this.

Alternatively (simpler + explicit): Remove the re-observe from `Scan`:
```rust
LoopStageEvent::Scan(_rs) => Ok(LoopStageResult::Noop),
```
The Tick path already ensures observe runs; re-running it on RouteSelected=observe is redundant.

---

## REPAIR-PLAN-FORMAT-1 — Enforce planner response schema to prevent `{signals, actions}` wrapper

**File:** planner prompt (in canon-agent-prompts or plan stage prompt builder)

**Root cause:**
The planner LLM returned a JSON object with shape `{"signals": {...}, "actions": [...]}`.
The expected format is a flat array of action objects (or an object with `actions` at top level
but no `signals` sibling — signals belong in LoopPlanned, not the plan response).

`parse_ok=false` at event 99, `valid_action_count=0` — the strict parser rejected the response.
The fallback parser rescued 4 actions from the nested format (events 105–108), so no data was lost,
but parse failures are a reliability hazard: fallback may misparse destructive actions.

**Fix:** Update the planner system prompt to specify the exact schema:
```
Return ONLY a JSON array of action objects. Do not wrap in a container object.
Do not include a "signals" key. Actions are objects with "action": "<kind>", plus kind-specific fields.

Correct:
[
  {"action": "run_command", "cmd": "...", "cwd": "..."},
  {"action": "done", "reason": "..."}
]

Incorrect (never do this):
{"signals": {...}, "actions": [...]}
```

---

## REPAIR-PLAN-CONTEXT-1 — Include target directory existence in observe payload

**File:** `canon-utils/canon-loop/src/stage/observe.rs`

**Root cause:**
`LoopObserved` carries `goal_text`, `error_count`, `compiler_errors` but not filesystem state.
The planner has no way to know whether the target project path exists.
At event 108, the planner emits `done` with reason "Project already initialized" — but
the goal explicitly states "The directory has been deleted. You have to start again."

**Fix:** In `observe::execute`, check if the goal text contains a `Target:` path block.
If found, stat the path and append a one-line workspace fact to the `LoopObserved` payload
(or to the context text used by the planner prompt):

```rust
// Pseudocode for observe.rs:
let target_exists = if let Some(path) = extract_target_path(&ctx.goal_text) {
    std::path::Path::new(&path).exists()
} else {
    true // unknown, assume exists
};
// Embed in LoopObserved or planner context:
// "target_path_exists=false"
```

Alternatively: add a `workspace_facts: Vec<String>` field to `LoopObserved` that can carry
ad-hoc key=value strings. The planner prompt should include these facts verbatim.

**Effect:** Planner sees `target_path_exists=false` and cannot hallucinate that the project
is already built. It will emit `cargo new` as the first action.

---

## REPAIR-PLAN-PREMATURE-DONE — Guard against "done" when prerequisite actions haven't run

**File:** `canon-utils/canon-loop/src/stage/plan.rs` (action validation / filtering)

**Root cause:**
The plan batch at events 105–108 contains:
1. `list_dir`
2. `run_command cargo build` (on non-existent path)
3. `apply_patch` to README.md
4. `done`

None of these create the project directory. The planner skipped `cargo new`. Then it included
`done` in the same batch — if `done` is acted on, the loop concludes before the missing steps
are discovered.

**Fix option A — Planner prompt rule:**
Add a rule: "Never include `done` in the same batch as other actions. `done` must be
the sole action in a batch, emitted only after `verify` has confirmed the goal is met."

**Fix option B — Code-level guard in plan.rs:**
When parsing a plan batch, if `done` is present alongside other actions, strip it:
```rust
if actions.iter().any(|a| a.action_kind == "done") && actions.len() > 1 {
    actions.retain(|a| a.action_kind != "done");
    // log: "stripped premature done from batch"
}
```

Both fixes are needed. Option A prevents the LLM from generating it; Option B is a safety net.

---

---

## REPAIR-VERIFY-1 — Don't mark workspace dirty for read-only actions

**File:** `canon-utils/canon-route/src/context.rs`

**Root cause (confirmed by source):**
`update_from_event` line 121: `self.workspace_dirty = true` and line 112: `self.acted_unverified = true`
are set unconditionally for every `LoopActed` event, including `list_dir` (a read-only action that
does not change the workspace). This causes the gate to route to `verify` after a `list_dir`,
which then finds the pre-existing project compiles, setting `finish_ready=true`.

**Evidence (run 3):**
- Event 1891: `LoopActed action_kind=list_dir` → `workspace_dirty=true, acted_unverified=true`
- Event 1907: Gate fires `acted_unverified=true requires verify` → routes to verify
- Event 1909: `LoopVerified compiler_clean=true` (pre-existing project, no work done)
- Loop concludes without creating the target project

**Fix:** Skip dirty flags for read-only actions:

```rust
// In update_from_event, LoopActed arm (context.rs ~line 110):
const READ_ONLY_ACTIONS: &[&str] = &["list_dir", "read_file", "search_files"];

RuntimeEvent::LoopActed(LoopActed { action_kind, .. }) => {
    self.planned_pending = self.planned_pending.saturating_sub(1);
    // Only mark workspace dirty / requiring verify for actions that actually mutate state.
    if !READ_ONLY_ACTIONS.contains(&action_kind.as_str()) {
        self.acted_unverified = true;
        self.workspace_dirty = true;
    }
    // ... rest unchanged ...
}
```

**Effect:** `list_dir` and other read-only actions no longer trigger verify. Only actual mutations
(`run_command`, `apply_patch`, `write_file`, etc.) set `acted_unverified=true`.

---

## REPAIR-VERIFY-2 — Fix false-positive goal satisfaction in `evaluate_goal_satisfied`

**File:** `canon-utils/canon-route/src/helpers.rs`

**Root cause (confirmed by source):**
```rust
pub fn evaluate_goal_satisfied(spec: Option<&GoalSpec>, workspace: &Path) -> bool {
    let Some(spec) = spec else { return false; };
    let required_loc = extract_loc_requirement(spec);
    required_loc == 0 || count_loc(workspace) >= required_loc  // ← BUG
}
```

When the goal contains no LOC requirement (`required_loc == 0`), the function returns `true`
unconditionally — even for a brand-new, untouched workspace. Almost all goals without explicit
LOC counts return `true`, so `finish_ready = compiler_clean && true = compiler_clean`.
The function never actually verifies that the goal's work was done.

**Evidence (run 3):**
- Goal: "Create a new Rust project from scratch. The directory has been deleted."
- No LOC requirement in goal spec → `required_loc = 0` → `evaluate_goal_satisfied = true`
- workspace/target dir already contained a Rust project from a prior run → `compiler_clean = true`
- `finish_ready = true` after the very first verify, before any `cargo new` was executed

**Fix:** Invert the default — only return `true` when we have affirmative evidence the goal is met.
When no criteria are present, return `false` (unsatisfied until proven otherwise):

```rust
pub fn evaluate_goal_satisfied(spec: Option<&GoalSpec>, workspace: &Path) -> bool {
    let Some(spec) = spec else {
        return false;
    };
    let required_loc = extract_loc_requirement(spec);
    // Only claim satisfied when we have a measurable criterion that IS met.
    // required_loc == 0 means no LOC criterion in goal — NOT evidence of satisfaction.
    if required_loc == 0 {
        return false;
    }
    count_loc(workspace) >= required_loc
}
```

**Longer-term fix:** Parse the `Target:` path block from `GoalSpec` and verify it exists and
contains at least one Rust source file. But the one-line change above eliminates the false positive
immediately and is safe: a `false` default means the loop continues planning until the LLM emits
`done` with an explicit reason, which is the correct termination signal for open-ended goals.

**Effect:** `finish_ready` stays `false` until the planner explicitly emits `done` (which is gated
on the `done` action being dispatched, not on a flawed filesystem heuristic).

---

## REPAIR-GATE-1 — Add hard `finish_ready=true → conclude` gate rule

**File:** `canon-utils/canon-judgment/src/lib.rs`

**Root cause (confirmed by source):**
The gate in `review()` has a rule that _blocks_ conclude when `!finish_ready` (line 371–375):
```rust
if lane == RouteKind::Conclude && !signals.finish_ready {
    lane = RouteKind::Verify;
    ...
}
```
But there is **no** rule that _forces_ conclude when `finish_ready=true`. The only conclude path
via signals requires `llm_signals.termination_readiness > 0.9 && llm_signals.verification_coverage >= 0.7`
(line 300–304) — which requires the LLM to self-report high termination readiness scores.

When `finish_ready=true` and the router LLM selects any lane other than conclude, the gate accepts
it without correction. The loop must wait for the next full LLM round-trip to arrive at conclude.

**Fix:** Add a hard gate rule immediately after the `delta_g` gate block:

```rust
// ── FINISH: hard conclude gate ─────────────────────────────────────────
// Once finish_ready=true and no queued plan remains, force conclude.
// This eliminates the extra LLM round-trip after verify sets finish_ready.
if signals.finish_ready && !signals.has_queued_plan {
    lane = RouteKind::Conclude;
    changed = true;
    notes.push("finish_ready=true → conclude");
}
```

**Placement:** Insert this block after line 313 (after the `delta_g` gate), before the
`minimum_confidence` check at line 316. This ensures it fires regardless of the LLM's pick,
but after all signal-driven rewrites have run.

**Why not conflict with the existing conclude-block rule?**
The existing rule (line 371–375) reads:
```rust
if lane == RouteKind::Conclude && !signals.finish_ready { ... }
```
With the new rule setting `lane = RouteKind::Conclude` only when `finish_ready=true`, the block
rule's condition `!signals.finish_ready` is false — the two rules are orthogonal.

**Effect:** After `finish_ready=true` is set by a verify pass, the next `RouteTick` will gate
to conclude without an LLM call. Removes one full router round-trip (typically 15–30 seconds)
from every successful run.

---

---

## REPAIR-PLAN-REPLAN-GUARD — Clear `last_planned_observed_tick` after any batch action

**File:** `canon-utils/canon-loop/src/executor.rs`

**Root cause (confirmed by source):**
`executor.rs` line 50: `if !a.success { self.ctx.last_planned_observed_tick = None; }` — only clears
the plan dedup guard on failure. After a successful action (e.g., `list_dir` success=true), the guard
stays set with the old observed tick.

With LIVE-1 dedup active, no new `LoopObserved` fires unless `error_count` or `goal_hash` changes.
`list_dir` success changes neither. So `last_planned_observed_tick` stays set, and `handle_observed`
returns `Noop` for every subsequent `RouteSelected=plan`:

```rust
// plan.rs line 216:
if ctx.pending_plan.is_some() || ctx.last_planned_observed_tick == Some(observed.tick) {
    return Ok(LoopStageResult::Noop);  // ← fires on every re-plan attempt
}
```

The router keeps selecting plan. The plan stage keeps returning Noop. `planned_pending` stays 0.
No forward progress. This is the **primary stall mechanism in run 4**.

**Evidence (run 4):**
- Event 2010: `LoopPlanned list_dir` (plan triggered successfully, dedup tick set)
- Event 2028: `LoopActed list_dir success=true` — guard NOT cleared
- Event 2043: `RouteSelected=plan` → plan trigger → Noop (same observed tick)
- Event 2046: route LLM fires again, selects plan again → same Noop cycle
- Events 2044–2053: only RouteTick events; no LoopPlanned ever fires again

**Fix:** Clear `last_planned_observed_tick` on any `LoopActed`, not just failure:

```rust
// executor.rs — LoopActed arm, replace:
if !a.success {
    self.ctx.last_planned_observed_tick = None;
}

// with:
self.ctx.last_planned_observed_tick = None;
```

**Safety:** Clearing on every LoopActed is safe because:
- If more actions are still queued: gate fires `has_queued_plan → act`, router never selects plan
- If a plan LLM is already in-flight: `pending_plan.is_some()` guard in `handle_observed` → Noop
- If batch is complete: allows the next plan call to incorporate accumulated `batch_acted` results

**Effect:** After each action completes, the plan stage is free to re-trigger on the next plan route
selection. The `batch_acted` buffer (list_dir stdout, cargo error messages, etc.) feeds directly into
the next plan call's prompt, enabling the planner to act on discovery results.

---

## REPAIR-PLAN-WORKSPACE-RULE — Fix incorrect cargo init guidance for existing Cargo project

**File:** `canon-utils/canon-loop/src/stage/plan.rs` (prompt, line ~505)

**Root cause (confirmed by source):**
The prompt's WORKSPACE RULE states:
```
WORKSPACE RULE: If the target project directory already exists in the workspace tree,
use `cargo init --name <name>` instead of `cargo new`. `cargo new` fails when the directory exists.
```

This is correct for "directory exists, no Cargo.toml". But when `Cargo.toml` already exists,
`cargo init` also fails with: `"there is already a Cargo.toml file in this directory"`.

The list_dir output shows `Cargo.toml README.md src/` in the target directory. The planner follows
the WORKSPACE RULE and plans `cargo init` → fails. The rule provides no recovery path for this case.

**Evidence (diff provided by user):**
```
+ System attempts `cargo init` on existing Cargo project → fails
+ Never retries with correct strategy (`cargo new` in parent)
```

**Fix:** Replace the single-case WORKSPACE RULE with a three-case decision tree:

```
WORKSPACE RULE (3 cases — check in order):
1. Target directory does NOT exist:
   → Use `cargo new <name>` with cwd set to the PARENT of the target directory.
   → Example: target=/workspace/proj → cwd=/workspace, cmd="cargo new proj"

2. Target directory EXISTS but has NO Cargo.toml:
   → Use `cargo init --name <name>` with cwd set to the target directory itself.
   → Example: target=/workspace/proj → cwd=/workspace/proj, cmd="cargo init --name proj"

3. Target directory EXISTS and ALREADY HAS Cargo.toml:
   → The project already exists. Do NOT run cargo new or cargo init.
   → If the goal says "start again" / "delete and recreate":
     Use run_command with cmd="rm -rf <target> && cargo new <name>" and cwd=<parent>.
     WARNING: rm -rf is BLOCKED. Instead: use apply_patch to delete src/*.rs, rewrite Cargo.toml.
   → If the goal says "add/modify existing project": proceed directly to apply_patch / run_command.

Use list_dir first to determine which case applies before choosing a cargo command.
```

**Effect:** Planner can correctly handle all three workspace states. `cargo init` is only used when
safe. For "start again" goals with an existing Cargo.toml, the planner uses apply_patch to reset
files rather than attempting blocked rm commands.

---

## Implementation order

1. **REPAIR-PLAN-REPLAN-GUARD** — **must fix first** (1-line change); currently the plan stage returns Noop for all plan routes after the first successful action, making every other fix irrelevant
2. **REPAIR-LIVE-3** — fix mid-batch deadlock; remaining batch actions never dispatched after first async action completes
3. **REPAIR-VERIFY-1** — prevents false verify trigger from read-only actions (list_dir)
4. **REPAIR-VERIFY-2** — fix false-positive goal satisfaction; eliminates spurious `finish_ready=true`
5. **REPAIR-GATE-1** — add hard conclude gate; removes extra router LLM round-trip after legitimate completion
6. **REPAIR-PLAN-WORKSPACE-RULE** — fix prompt's cargo strategy for existing-Cargo.toml case
7. **REPAIR-LIVE-1** — eliminates observe spam; reduces LLM context waste and noise events
8. **REPAIR-LIVE-2** — covered by LIVE-1 deduplication; explicit `Scan → Noop` is a 1-liner
9. **REPAIR-PLAN-FORMAT-1** — prompt schema fix; prevents recurring `parse_ok=false` on every plan call
10. **REPAIR-PLAN-PREMATURE-DONE** — code guard strips `done` from multi-action batches
11. **REPAIR-PLAN-CONTEXT-1** — observe filesystem fact; unblocks planner from hallucinating project state

---

## Verification

After applying fixes, a healthy trace should show:
- All actions in a plan batch execute sequentially (list_dir → run_command → apply_patch → done)
- No stall after an async action completes with more actions still queued
- `list_dir` does NOT set `workspace_dirty=true` or trigger a verify route
- `finish_ready` stays `false` until the planner explicitly emits `done` (no LOC requirement goals)
- No false-positive conclude before target directory is created
- Gate routes to conclude immediately after `finish_ready=true` without an extra LLM call
- After each successful action, a subsequent `RouteSelected=plan` triggers a new plan LLM call (not Noop)
- Planner uses correct cargo strategy based on whether Cargo.toml exists (not just whether dir exists)
- At most 1–2 `LoopObserved` events before the first `RouteSelected`
- No `LoopObserved` events with identical `(error_count, goal_hash)` back-to-back
- `parse_ok=true` on all planner responses
- `LoopPlanned` batch contains `cargo new` before `cargo build` when target path does not exist
- `done` action only appears as a singleton, never in a multi-action batch

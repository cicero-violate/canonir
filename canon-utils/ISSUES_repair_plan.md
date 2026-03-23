# Canon Runtime — Issues Repair Plan

Repair plans corresponding to issues in `ISSUES.md`. Each plan identifies the root cause,
the exact file and line to change, and the expected outcome. Plans are ordered by severity
then dependency.

---

## REPAIR-1 — Router Fires Before Goal Is Known

**Fixes**: ISSUE-1
**File**: `canon-utils/canon-route/src/executor.rs`
**Function**: `try_dispatch_route`

### Root Cause

`try_dispatch_route` is called on every idle `RouteTick`. On the first tick, `LoopObserved`
has not yet fired, so `ctx.context_ready = false` and `ctx.mission_summary` is empty. The
router dispatches an LLM call with `Mission: (unknown goal)` and wastes an LLM round-trip
making a blind routing decision.

### Fix

Add a `context_ready` guard at the top of `try_dispatch_route`, before the `pending_request_id`
check:

```rust
fn try_dispatch_route(&mut self) {
    if self.ctx.halted { return; }
    if !self.ctx.context_ready { return; }   // <-- add this
    if self.pending_request_id.is_some() { return; }
    // ...
}
```

### Expected Outcome

No router LLM call fires until `LoopObserved` has delivered a goal and set
`context_ready = true`. First `RouteSelected` always has a populated `Mission:` block.

---

## REPAIR-2 — Mission Summary Is a Machine ID, Not a Goal Description

**Fixes**: ISSUE-2
**File**: `canon-utils/canon-goal/src/lib.rs`
**Functions**: `parse_agent_goal_markdown`, `summarize_goal`

### Root Cause

`parse_agent_goal_markdown` only parses `- Project path:` lines and `- ` bullet points. The
free-form description block below the `# Agent Goal` heading (e.g. "Here create another new
test Rust project in...") is silently dropped. `GoalSpec.id` defaults to `"agent_goal"`, so
`summarize_goal` opens with `goal_id=agent_goal` — a meaningless constant.

`RouteContext.mission_summary` is set from `summarize_goal`, so every router prompt starts
with this opaque line.

### Fix

**Step 1** — Add a `description: String` field to `GoalSpec`.

**Step 2** — In `parse_agent_goal_markdown`, collect the free-form text that appears between
the `# Agent Goal` heading and the first `##` sub-heading into `spec.description`.

```rust
// Pseudocode for the new parsing loop
let mut in_description = false;
let mut description_lines: Vec<&str> = Vec::new();
for line in goal_text.lines() {
    let trimmed = line.trim();
    if trimmed.starts_with("# ") {
        in_description = true;
        continue;
    }
    if trimmed.starts_with("## ") {
        in_description = false;
    }
    if in_description && !trimmed.is_empty() {
        description_lines.push(trimmed);
    }
    // existing - Project path: and - bullet parsing unchanged
}
spec.description = description_lines.join(" ");
```

**Step 3** — Update `summarize_goal` to lead with the description:

```rust
pub fn summarize_goal(spec: &GoalSpec) -> String {
    let desc = if spec.description.is_empty() { "(no description)".to_string() }
               else { spec.description.clone() };
    let target = ...; // unchanged
    let requirements = ...; // unchanged
    format!("{desc}\ntarget: {target}\nrequirements:\n{requirements}")
}
```

### Expected Outcome

Router prompt `Mission:` block reads: `"Here create another new test Rust project in
/workspace/ai_sandbox/canon/test_rust_project_v3"` followed by the requirements — giving
the LLM enough context to make a meaningful routing decision.

---

## REPAIR-3 — LoopRewarded halt=False When Compiler Is Clean

**Fixes**: ISSUE-3
**File**: `canon-utils/canon-loop/src/stage/reward.rs`
**Function**: `execute` (the one called from `LoopVerified`, not `execute_conclude`)

### Root Cause

```rust
// current — line 25
let halt = !v.compiler_clean || ctx.stagnant_ticks > 10;
```

This is semantically inverted. It evaluates to `true` (halt) when the compiler is **dirty**,
and `false` (continue) when clean. The loop therefore terminates on compile failure and
continues forever on compile success.

The "conclude" route path (`execute_conclude`) is the correct place to halt on goal
satisfaction — it always returns `halt: true`. The auto-reward triggered by `LoopVerified`
should only halt on **stagnation** (stuck with no progress), not on clean state.

### Fix

```rust
// fixed — reward.rs::execute
let halt = ctx.stagnant_ticks > 10;
```

Remove the `!v.compiler_clean` term entirely. The stagnation guard (> 10 ticks without
compiler improvement) remains as a safety timeout. Goal-satisfied halt is handled exclusively
by `execute_conclude`.

### Expected Outcome

After `LoopVerified { compiler_clean: true }`, `LoopRewarded { halt: false }` fires (the
auto-reward continues the loop). The loop only halts via the conclude route, where
`execute_conclude` sets `halt: true`.

---

## REPAIR-4 — Router Keeps Selecting plan After Goal Is Satisfied

**Fixes**: ISSUE-4 (the LLM routing half of the infinite observe loop)
**File**: `canon-utils/canon-decision/src/lib.rs`
**Function**: `compose_routing_prompt`

### Root Cause

The router prompt's "conclude" route description doesn't connect to the `finish_ready` flag
in the snapshot. The LLM sees `finish_ready=true` in the snapshot but the route descriptions
don't say "select conclude when finish_ready=true". The LLM keeps selecting "plan" because
"context ready, no pending actions → plan" is a stronger heuristic signal than the
`finish_ready` flag.

### Fix

Update the conclude route description in `route_descriptions`:

```rust
// current
"- conclude: finish when the goal is satisfied (calls reward::execute_conclude)."

// fixed
"- conclude: select this when finish_ready=true in the snapshot. \
   This terminates the loop. Only select conclude when the workspace is verified and \
   the goal requirements are met."
```

Also add an explicit rule above the route list:

```
ROUTING RULE: If the snapshot shows finish_ready=true, you MUST select conclude. \
Do not select plan or act when finish_ready=true.
```

### Expected Outcome

When `finish_ready=true` appears in the snapshot, the router LLM selects "conclude", which
triggers `execute_conclude` → `LoopRewarded { halt: true }` → loop stops.

---

## REPAIR-5 — no_op LoopPlanned Blocks Router Forever

**Fixes**: ISSUE-5 (the `planned_pending` half of the router going dark)
**File**: `canon-utils/canon-route/src/context.rs`
**Function**: `update_from_event` — `LoopPlanned` arm

### Root Cause

Every `LoopPlanned` event increments `planned_pending` by 1. `planned_pending` is only
decremented by `LoopActed`. When the planner emits `LoopPlanned { action_kind: "no_op" }`,
no `LoopActed` ever fires for it. `planned_pending` is permanently 1, and the idle check in
`try_dispatch_route` (`planned_pending == 0 && pending_tool_result_ids.is_empty()`) never
passes again. The router goes silent for all subsequent ticks.

### Fix

Skip the increment for `no_op` actions:

```rust
RuntimeEvent::LoopPlanned(LoopPlanned { action_kind, .. }) => {
    if action_kind != "no_op" {                        // <-- add this guard
        self.planned_pending = self.planned_pending.saturating_add(1);
    }
    // ... rest of handler unchanged
}
```

### Expected Outcome

A `no_op` plan does not alter `planned_pending`. The idle check passes on the next
`RouteTick`, the router dispatches its next LLM call, and routing continues normally.

---

## REPAIR-6 — Planner Picks `cargo new` on Existing Directory

**Fixes**: ISSUE-6
**File**: `canon-utils/canon-loop/src/stage/plan.rs`
**Function**: `build_prompt` (or the function that assembles the planner LLM prompt)

### Root Cause

The planner has a workspace file tree in the prompt (added previously), but the LLM still
picks `cargo new` even when the target directory exists. The workspace tree shows the
directory, but the LLM has no explicit instruction distinguishing `cargo new` (creates dir)
from `cargo init` (initialises in-place).

### Fix

Add an explicit rule to the planner prompt's constraints section:

```
WORKSPACE RULE: If the target project directory already exists in the workspace tree,
use `cargo init --name <name>` (not `cargo new`). `cargo new` fails if the directory
exists. `cargo init` initialises a Cargo project inside an existing directory.
```

This is a prompt-only change — no struct changes needed.

### Expected Outcome

When the workspace tree shows `/workspace/.../test_rust_project_v3/` already exists, the
planner emits `run_command("cargo init --name test_rust_project_v3 test_rust_project_v3")`
instead of `cargo new`.

---

## REPAIR-7 — Planner Has No Feedback That Destructive Commands Are Blocked

**Fixes**: ISSUE-7
**File**: `canon-utils/canon-loop/src/stage/plan.rs`
**Function**: `build_prompt`

### Root Cause

The planner prompt instructs the LLM not to plan destructive commands, but the instruction
is vague ("do not emit destructive commands"). The act stage blocks them at runtime, but
the LLM has no knowledge of the exact block list. It keeps including them because there is
no signal closing the loop — `LoopActed { stderr: "rejected_destructive_command" }` exists
in `recent_tool_results`, but the planner doesn't emphasise the significance of this stderr
value.

### Fix

**Step 1** — Add the explicit deny-list to the planner prompt:

```
SAFETY RULE: The following commands are BLOCKED and will always fail. Do NOT plan them:
rm -rf, git reset --hard, git clean -f, dd if=, mkfs, shred, >/dev/sd
```

**Step 2** — When `recent_tool_results` contains a result with
`stderr = "rejected_destructive_command"`, inject a warning line near the top of the prompt:

```
WARNING: The previous plan included a blocked destructive command. It was not executed.
Do not retry it.
```

### Expected Outcome

The planner never produces destructive commands. If it does, the warning on the next
planning cycle prevents re-inclusion.

---

## REPAIR-8 — Single Failure Aborts All Remaining Batch Actions

**Fixes**: ISSUE-8
**File**: `canon-utils/canon-loop/src/stage/act.rs` (or wherever batch abort is triggered)

### Root Cause

When one action in a multi-action batch fails, all remaining queued actions for the same
batch emit `LoopActed { success: false, stderr: "skipped:batch_aborted" }`. Independent
actions (e.g. creating README.md after `cargo new` failed) are voided even though they
could proceed.

### Fix

Change batch-abort to only skip actions that are sequentially dependent on the failed one.
The dependency is encoded in ordering: actions are aborted only if they were emitted in the
same `LoopPlanned` batch AND come after the failed action in that batch's sequence.

Introduce a per-action `is_independent: bool` flag in `LoopPlanned.action_payload` (or
infer independence by action kind):
- `list_dir`, `read_file`, `write_file`, `apply_patch` are independent — do not abort.
- `run_command` with a build/install step is dependent — abort on prior failure.

Until dependency tracking is implemented, a simpler intermediate fix: only abort on
failure of `run_command` actions, not on `write_file` or `apply_patch` failures. This
prevents a single shell command failure from voiding file-write actions.

**File**: Look for the batch-abort sentinel (`"skipped:batch_aborted"`) in `act.rs` and
change the condition from "any prior failure" to "prior run_command failure".

### Expected Outcome

A failed `cargo new` does not abort subsequent `write_file` (README.md) or `apply_patch`
actions. The planner's next cycle has partial progress to build on.

---

## REPAIR-9 — Capture Invariant Violations Should Fail Fast

**Fixes**: ISSUE-9
**File**: `rustc_capture` crate (in test_projects capture pipeline)

### Root Cause

Invariant violations in the path interner and name interner are logged with
`capture failed: Invariant violation: ...` but the capture continues writing the JSON and
exits 0. The downstream orchestration has no way to know the IR is potentially corrupt.

### Fix

Change the invariant-violation handler in `rustc_capture` to:
1. Accumulate violations into an `errors: Vec<String>` counter.
2. After compilation completes, if `errors.len() > 0`:
   - Append an `"invariant_violations": [...]` array to the capture JSON.
   - Exit with a non-zero code (e.g. `exit(1)`).

This makes the capture pipeline fail visibly rather than silently producing corrupt IR.

The orchestration script should check the exit code and skip orchestration + emission if
capture failed.

---

## REPAIR-10 — FAILED Build Reports Zero Errors

**Fixes**: ISSUE-10
**File**: Orchestration pipeline — the script or binary that runs `cargo build` and writes
the build report JSON.

### Root Cause

`cargo build` exits non-zero but the build report captures `error_count: 0, warning_count:
0`. The reporter is only counting structured diagnostics (JSON lines with `"level": "error"`
in `cargo build --message-format json`). A build failure caused by non-diagnostic reasons
(missing Cargo.toml, linker error, workspace misconfiguration) doesn't produce structured
JSON diagnostics and so registers as 0 errors.

### Fix

Two changes:

1. When `cargo build` exits non-zero AND `error_count == 0`, capture the raw stderr output
   and store it as `build_stderr: String` in the build report JSON. This is the fallback
   for non-diagnostic failures.

2. The `build_report.json` schema should include:
   ```json
   {
     "result": "FAILED",
     "error_count": 0,
     "warning_count": 0,
     "build_stderr": "error: could not compile `canon` ...\n..."
   }
   ```

### Expected Outcome

Any build failure — including structural ones with 0 compiler diagnostics — has its cause
captured in the report. Downstream debugging and automated repair can act on the `build_stderr`
field.

---

## REPAIR-11 — Emitter Produces goto Placeholder (Build-Breaking)

**Fixes**: ISSUE-11
**File**: Canon IR emitter — the control-flow emission path for `goto`-like constructs.

### Root Cause

The structural surface scanner reports `// goto count: 1`, meaning the emitter outputs a
`// goto` comment where it should emit valid Rust control flow. This is an unimplemented
branch in the emitter for a specific MIR construct (likely a non-local jump or labeled
break). The `// goto` placeholder is a comment, not valid Rust, causing the emitted
crate to fail to compile (ISSUE-10).

### Fix

Locate the emitter branch that emits `// goto`. Replace with one of:
- A `todo!()` macro (makes the build fail loudly with a clear message)
- A valid equivalent Rust construct if the semantic is known (e.g. `break 'label` for
  labeled loops, `continue` for loop-back jumps)

If the construct has no direct Rust equivalent, emit `unimplemented!("goto: {label}")` so
the crate at least compiles (with a runtime panic if reached) rather than failing at
compile time.

---

## REPAIR-12 — Name Shadowing in Provenance Solver

**Fixes**: ISSUE-12
**File**: Orchestration — `provenance_solver` module

### Root Cause

10 instances of `CanonId`, `NameId`, `PathId` shadowed within the same module in the IR.
The shadowing comes from two sources:
1. **Capture invariant violations (ISSUE-9)**: malformed path segments produce duplicate
   nodes with the same name in the same module scope.
2. **Legitimate re-exports**: the canon crate re-exports its own types, producing multiple
   nodes with the same qualified path in the solver's view.

### Fix

**Short-term** (provenance_solver): When a name collision is detected, keep the first
registration and log the second as a warning (current behaviour). Annotate the output IR
node with a `shadowed: true` flag so emission can handle it explicitly.

**Long-term** (capture): Fixing ISSUE-9 (invariant violations fail fast) will eliminate the
malformed-path duplicates, reducing shadowing warnings to only legitimate re-export cases.
Those re-export cases can then be handled by de-duplicating nodes whose qualified path
and type are identical.

---

## Summary — Priority Order

| Repair   | Fixes    | File(s)                                   | Effort | Priority  |
|----------|----------|-------------------------------------------|--------|-----------|
| REPAIR-3 | ISSUE-3  | `canon-loop/stage/reward.rs`              | XS     | Critical  |
| REPAIR-5 | ISSUE-5  | `canon-route/context.rs`                  | XS     | Critical  |
| REPAIR-1 | ISSUE-1  | `canon-route/executor.rs`                 | XS     | High      |
| REPAIR-4 | ISSUE-4  | `canon-decision/lib.rs`                   | XS     | High      |
| REPAIR-2 | ISSUE-2  | `canon-goal/lib.rs`                       | S      | High      |
| REPAIR-6 | ISSUE-6  | `canon-loop/stage/plan.rs`                | XS     | High      |
| REPAIR-7 | ISSUE-7  | `canon-loop/stage/plan.rs`                | XS     | Medium    |
| REPAIR-8 | ISSUE-8  | `canon-loop/stage/act.rs`                 | M      | Medium    |
| REPAIR-9 | ISSUE-9  | `rustc_capture`                           | S      | High      |
| REPAIR-10| ISSUE-10 | orchestration build reporter              | S      | High      |
| REPAIR-11| ISSUE-11 | canon IR emitter                          | M      | Medium    |
| REPAIR-12| ISSUE-12 | provenance_solver                         | M      | Low       |

XS = < 1 hour, S = half day, M = 1–2 days

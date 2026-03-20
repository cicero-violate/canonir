# IMPLEMENTATION_PLAN.md — Goal Enforcement and Shape Improvement

Source: `/workspace/ai_sandbox/canon/canon-utils/PLAN.md`

## Diagnosis

Three compounding failures prevent the agent from building 50k+ LOC:

**Failure 1 — Goal check is wrong**
`evaluate_goal_satisfied` (event_runtime.rs) only checks:
- target dir exists
- README.md non-empty
- `cargo build` exits 0

It ignores "50000+ LOC" entirely. The moment `cargo build` passes on a 10-line Hello
World, `finish_ready=true`, the gate allows `conclude`, and the session halts.

**Failure 2 — Done guard silences the planner**
`PlanConsumer.handle_observed` (canon-plan/src/lib.rs lines 165–181):
```rust
if error_count == 0 && last_done_goal == observed.goal_text {
    emit no_op (reason: "goal_complete");
    return;
}
```
The LLM says `{"done": true}` after creating a minimal project. `last_done_goal` is set.
Every subsequent shape tick hits this guard and emits `no_op` — the LLM is never called
again. The agent has permanently stopped shaping.

**Failure 3 — Planner has no information**
`build_prompt` (canon-plan/src/lib.rs lines 512–577) includes:
- Goal text
- Compiler errors
- Previous action results

It does NOT include:
- Current LOC count
- Which requirements are met / unmet
- Any instruction to generate large volumes of code per step

The LLM has no idea it is at 50 LOC instead of 50000.

---

## Files changed

| File | Change |
|------|--------|
| `canon-utils/canon-runtime/src/bin/event_runtime.rs` | Fix `evaluate_goal_satisfied` to count LOC; add `count_loc` helper |
| `canon-utils/canon-plan/src/lib.rs` | Add `workspace: PathBuf` to `PlanConsumer`; fix done guard; add LOC + requirement progress to `build_prompt` |
| `canon-utils/canon-runtime/src/bin/event_runtime.rs` | Pass `workspace` to `PlanConsumer::new` |

---

## Change 1 — `event_runtime.rs`: Fix `evaluate_goal_satisfied`

### Add `count_loc` helper

Add directly above `evaluate_goal_satisfied` (before line 267):

```rust
fn count_loc(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut total = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += count_loc(&path);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                total += content.lines().count();
            }
        }
    }
    total
}

fn extract_loc_requirement(spec: &GoalSpec) -> usize {
    for req in &spec.requirements {
        // Match patterns like "50000+ LOC" or "50000 LOC" or "50k LOC"
        let lower = req.to_lowercase();
        if lower.contains("loc") {
            let digits: String = req.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<usize>() {
                return n;
            }
        }
    }
    0
}
```

### Replace `evaluate_goal_satisfied` body

Current body (lines 268–275):
```rust
let Some(spec) = spec else {
    return false;
};
let target = spec.target_path.clone().unwrap_or_else(|| workspace.join("test_rust_project_v3"));
if !target.is_dir() {
    return false;
}

let readme = target.join("README.md");
let readme_non_empty = std::fs::metadata(&readme).ok().map(|m| m.is_file() && m.len() > 0).unwrap_or(false);
if !readme_non_empty {
    return false;
}

std::process::Command::new("cargo").arg("build").current_dir(&target).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
```

Replace with:
```rust
let Some(spec) = spec else {
    return false;
};
let target = spec.target_path.clone().unwrap_or_else(|| workspace.join("test_rust_project_v3"));
if !target.is_dir() {
    return false;
}

let readme = target.join("README.md");
let readme_non_empty = std::fs::metadata(&readme).ok().map(|m| m.is_file() && m.len() > 0).unwrap_or(false);
if !readme_non_empty {
    return false;
}

let required_loc = extract_loc_requirement(spec);
if required_loc > 0 {
    let actual_loc = count_loc(&target);
    if actual_loc < required_loc {
        return false;
    }
}

std::process::Command::new("cargo").arg("build").current_dir(&target).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
```

---

## Change 2 — `canon-plan/src/lib.rs`: Add `workspace` to `PlanConsumer`

### Add field to struct

Current `PlanConsumer` struct (lines 10–24):
```rust
pub struct PlanConsumer {
    emitter: Option<EventEmitterHandle>,
    pending: Option<PendingPlan>,
    last_observed: Option<LoopObserved>,
    last_planned_observed_tick: Option<u64>,
    last_done_goal: Option<String>,
    batch_acted: Vec<LoopActed>,
    batch_tool_results: Vec<ToolResult>,
    last_prompted_goal: Option<String>,
}
```

Add `workspace` field:
```rust
pub struct PlanConsumer {
    emitter: Option<EventEmitterHandle>,
    pending: Option<PendingPlan>,
    last_observed: Option<LoopObserved>,
    last_planned_observed_tick: Option<u64>,
    last_done_goal: Option<String>,
    batch_acted: Vec<LoopActed>,
    batch_tool_results: Vec<ToolResult>,
    last_prompted_goal: Option<String>,
    workspace: std::path::PathBuf,
}
```

### Change `new()` signature and body

Current (lines 41–53):
```rust
pub fn new() -> Self {
    Self {
        emitter: None,
        pending: None,
        last_observed: None,
        last_planned_observed_tick: None,
        last_done_goal: None,
        batch_acted: Vec::new(),
        batch_tool_results: Vec::new(),
        last_prompted_goal: None,
    }
}
```

Replace with:
```rust
pub fn new(workspace: std::path::PathBuf) -> Self {
    Self {
        emitter: None,
        pending: None,
        last_observed: None,
        last_planned_observed_tick: None,
        last_done_goal: None,
        batch_acted: Vec::new(),
        batch_tool_results: Vec::new(),
        last_prompted_goal: None,
        workspace,
    }
}
```

---

## Change 3 — `canon-plan/src/lib.rs`: Fix the done guard

### Add LOC-check helper to `impl PlanConsumer`

Add this method to the `impl PlanConsumer` block (before `check_llm_timeout`):

```rust
/// Returns true only if the workspace actually satisfies the LOC requirement
/// parsed from the active goal text. Used to prevent the done-guard from
/// silencing the planner when the LLM prematurely declares "done".
fn requirements_satisfied(&self, observed: &LoopObserved) -> bool {
    let Some(goal_text) = observed.goal_text.as_ref() else {
        return false;
    };
    // Parse LOC requirement from goal text lines
    let required_loc = goal_text
        .lines()
        .filter(|l| l.to_lowercase().contains("loc"))
        .find_map(|l| {
            let digits: String = l.chars().filter(|c| c.is_ascii_digit()).collect();
            digits.parse::<usize>().ok()
        })
        .unwrap_or(0);
    if required_loc == 0 {
        return true; // No LOC requirement — trust the LLM
    }
    let actual_loc = count_loc_in_workspace(&self.workspace);
    actual_loc >= required_loc
}
```

Add `count_loc_in_workspace` as a module-level private function (not a method),
below the existing private helpers at the bottom of the file:

```rust
fn count_loc_in_workspace(workspace: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(workspace) else { return 0 };
    let mut total = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += count_loc_in_workspace(&path);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                total += content.lines().count();
            }
        }
    }
    total
}
```

### Fix the done guard in `handle_observed`

Current guard (lines 165–181):
```rust
// Goal already completed — don't call the LLM again unless errors appear.
if observed.error_count == 0 && self.last_done_goal.is_some() && self.last_done_goal == observed.goal_text {
    self.emit_plan(LoopPlanned {
        tick: observed.tick,
        action_kind: "no_op".to_string(),
        action_payload: serde_json::json!({}),
        reason: "goal_complete".to_string(),
        ...
    });
    return;
}
```

Replace with:
```rust
// Only suppress LLM call if the LLM previously declared done AND requirements
// are actually satisfied in the workspace. If LOC requirements are unmet,
// reset the guard so the LLM is called again with updated context.
if observed.error_count == 0 && self.last_done_goal.is_some() && self.last_done_goal == observed.goal_text {
    if self.requirements_satisfied(observed) {
        self.emit_plan(LoopPlanned {
            tick: observed.tick,
            action_kind: "no_op".to_string(),
            action_payload: serde_json::json!({}),
            reason: "goal_complete".to_string(),
            llm_request_id: None,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
        });
        return;
    }
    // Requirements not met — reset guard, fall through to call LLM again.
    self.last_done_goal = None;
}
```

### Fix `last_done_goal` assignment in `handle_capability_completed`

Current (lines 360–363, inside `LlmAction::Done` arm):
```rust
LlmAction::Done { reason } => {
    self.last_done_goal = pending.goal_text.clone();
    ...
}
```

Replace with:
```rust
LlmAction::Done { reason } => {
    // Only lock in the done guard if requirements are actually satisfied.
    // If not, we let next shape tick call the LLM again with current context.
    if let Some(goal_text) = &pending.goal_text {
        // Reuse requirements_satisfied via a temporary observed-like check
        let required_loc = goal_text
            .lines()
            .filter(|l| l.to_lowercase().contains("loc"))
            .find_map(|l| {
                let digits: String = l.chars().filter(|c| c.is_ascii_digit()).collect();
                digits.parse::<usize>().ok()
            })
            .unwrap_or(0);
        let satisfied = required_loc == 0 || count_loc_in_workspace(&self.workspace) >= required_loc;
        if satisfied {
            self.last_done_goal = pending.goal_text.clone();
        }
        // If not satisfied: last_done_goal stays None/old, LLM called again next tick.
    } else {
        self.last_done_goal = pending.goal_text.clone();
    }
    ...
}
```

---

## Change 4 — `canon-plan/src/lib.rs`: Add progress context to `build_prompt`

### Change `build_prompt` signature

Current:
```rust
fn build_prompt(observed: &LoopObserved, batch_acted: &[LoopActed], batch_tool_results: &[ToolResult], include_full_goal: bool) -> String {
```

Add `workspace` parameter:
```rust
fn build_prompt(observed: &LoopObserved, batch_acted: &[LoopActed], batch_tool_results: &[ToolResult], include_full_goal: bool, workspace: &std::path::Path) -> String {
```

### Update the call site in `handle_observed` (line 189):

```rust
// Current:
let prompt = build_prompt(observed, &self.batch_acted, &self.batch_tool_results, include_full_goal);

// After:
let prompt = build_prompt(observed, &self.batch_acted, &self.batch_tool_results, include_full_goal, &self.workspace);
```

### Add progress section inside `build_prompt`

Add after `goal_section` is computed (after line 517), before `error_section`:

```rust
let progress_section = build_progress_section(observed, workspace);
```

Add `build_progress_section` as a module-level private function:

```rust
fn build_progress_section(observed: &LoopObserved, workspace: &std::path::Path) -> String {
    let Some(goal_text) = observed.goal_text.as_ref() else {
        return String::new();
    };

    // Parse LOC requirement
    let required_loc: usize = goal_text
        .lines()
        .filter(|l| l.to_lowercase().contains("loc"))
        .find_map(|l| {
            let digits: String = l.chars().filter(|c| c.is_ascii_digit()).collect();
            digits.parse::<usize>().ok()
        })
        .unwrap_or(0);

    if required_loc == 0 {
        return String::new();
    }

    // Find target path from goal text
    let target = goal_text
        .lines()
        .find_map(|l| l.trim().strip_prefix("- Project path:").map(|p| std::path::PathBuf::from(p.trim().trim_matches('`'))))
        .unwrap_or_else(|| workspace.join("test_rust_project_v3"));

    let actual_loc = count_loc_in_workspace(&target);
    let remaining = required_loc.saturating_sub(actual_loc);
    let pct = if required_loc > 0 { (actual_loc * 100) / required_loc } else { 100 };

    format!(
        "## LOC Progress\nCurrent: {} lines / {} required ({}%)\nRemaining: {} lines to write\n\n",
        actual_loc, required_loc, pct, remaining
    )
}
```

### Inject `progress_section` into the final format string

Current format string at line 570:
```rust
format!(
    "{goal}{last_action}{last_tool_result}{errors}Execution policy constraints:\n...",
    goal = goal_section,
    last_action = last_action_section,
    last_tool_result = last_tool_result_section,
    errors = error_section,
)
```

Change to:
```rust
format!(
    "{goal}{progress}{last_action}{last_tool_result}{errors}Execution policy constraints:\n- Do NOT emit destructive commands (`rm -rf`, `git reset --hard`, `git clean -f`, `dd`, `mkfs`, `shred`).\n- If a target directory already exists, prefer `cargo init --bin <dir>` instead of deleting and recreating it.\n\nGeneration policy:\n- You MUST generate LARGE amounts of code per shape step. Each `write` action should contain hundreds to thousands of lines of Rust.\n- A single shape step should advance LOC by thousands, not tens. Write entire modules at once.\n- Do not declare done until the LOC progress section shows >= required LOC.\n\nReturn one or more fenced ```json code blocks (no prose outside code blocks). Each block must be one action object using one schema:\n- Run a command:  {{\"cmd\": \"cargo new foo\", \"cwd\": \"/path\"}}\n- Write a file:   {{\"write\": \"/abs/path\", \"content\": \"full content\"}}\n- Patch a file:   {{\"path\": \"/abs/path\", \"old\": \"exact text\", \"new\": \"replacement\"}}\n- Signal done:    {{\"done\": true, \"reason\": \"...\"}}",
    goal = goal_section,
    progress = progress_section,
    last_action = last_action_section,
    last_tool_result = last_tool_result_section,
    errors = error_section,
)
```

---

## Change 5 — `event_runtime.rs`: Pass workspace to `PlanConsumer::new`

Current (line 655 approx):
```rust
Box::new(PlanConsumer::new()),
```

Replace with:
```rust
Box::new(PlanConsumer::new(workspace.clone())),
```

`workspace` is already defined at line 652:
```rust
let workspace = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
```

---

## Expected behaviour after changes

```
shape tick fires
  → PlanConsumer receives route_selected(shape)
  → handle_observed called
  → last_done_goal guard: previous "done" was set, BUT count_loc = 1200 < 50000
    → guard resets, falls through to LLM call
  → build_prompt includes:
      "## LOC Progress
       Current: 1200 lines / 50000 required (2%)
       Remaining: 48800 lines to write"
      "Generation policy: you MUST generate LARGE amounts of code per shape step..."
  → LLM sees 2% progress, generates 10+ write actions with large Rust modules
  → PlanConsumer emits 10+ LoopPlanned events (one per action block)
  → ActConsumer executes them sequentially
  → LoopVerified fires
  → evaluate_goal_satisfied: count_loc = 12000 < 50000 → false
  → finish_ready stays false → gate blocks conclude
  → next tick: shape again, LLM sees 24% progress, generates more code
  → ... repeats until count_loc >= 50000 AND cargo build passes
  → evaluate_goal_satisfied returns true → finish_ready = true → conclude
```

---

## What does NOT change

| Component | Why |
|-----------|-----|
| `VerifyConsumer` | `run_cargo_check` still validates compilation — unchanged |
| `evaluate_goal_satisfied` cargo build check | Still required; LOC check is additive |
| Routing gate (`canon-judgment`) | Gate rules unchanged; `finish_ready` signal now just triggers correctly |
| `RewardConsumer` | Halt-on-done logic unchanged; halts only when system actually concludes |
| `ObserveConsumer` | Unchanged |
| Action types (patch/write/command/done) | Parser unchanged; more actions per plan batch just means more `LoopPlanned` events |

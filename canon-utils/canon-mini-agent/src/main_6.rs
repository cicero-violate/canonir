use anyhow::{anyhow, bail, Context, Result};
use canon_llm::{
    config::{CapabilityConfig, LlmEndpoint},
    endpoint_worker::{llm_worker_new_tabs, llm_worker_send_request},
    tab_management::TabManagerHandle,
    ws_server,
    ws_server::WsBridge,
};
use canon_tools_patch::apply_patch;
use serde_json::json;
use serde_json::Value;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};

const WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const SPEC_FILE: &str = "PLANS/SPEC.md";
const EXECUTOR_A_PLAN_FILE: &str = "PLANS/executor-a.md";
const EXECUTOR_B_PLAN_FILE: &str = "PLANS/executor-b.md";
const DIAGNOSTICS_FILE: &str = "PLANS/diagnostics.md";
const ACTION_LOG_FILE: &str = "/workspace/ai_sandbox/canon/agent_logs/mini_agent_actions.jsonl";
const WS_PORT_DEFAULT: u16 = 9103;
const MAX_STEPS: usize = 2000;
const MAX_FULL_READ_LINES: usize = 500;
const MAX_SNIPPET: usize = 3000;

const SYSTEM_INSTRUCTIONS_EXECUTOR: &str = r#"You are the canon mini-agent-executor.

Your job is to execute the highest-priority READY work described in the lane plan provided to you.
`PLANS/SPEC.md` is the canonical contract.
The planner owns `PLANS/executor-a.md` and `PLANS/executor-b.md`.
The verifier judges code against `PLANS/SPEC.md`.
You should only work on the top 1-5 ready tasks in the current cycle, then yield.
Do not reorganize or update `PLANS/SPEC.md`, `PLANS/executor-a.md`, or `PLANS/executor-b.md` yourself.
Make source changes, run checks, and report evidence in `done.reason`.

Canonical law:
- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- `scheduler_len`, `planned_pending`, and other queue-like counters are derived telemetry unless the code proves otherwise.
- Do not preserve or introduce routing logic that depends on local mirrors when semantic-state facts are available.
- Prefer changes that make code follow:
  state -> decision -> transition

You work inside the canon workspace at /workspace/ai_sandbox/canon. All relative file paths resolve against this workspace root.

Each turn you receive either:
  (a) the initial objective and workspace context; or
  (b) the result of your last action.

You respond with exactly one action per turn, as a single JSON object wrapped in a `json` code block.
Available actions:
- `done`
- `list_dir`
- `read_file`
- `apply_patch`
- `run_command`
- `python`
Every action MUST include:
- `observation`: what you can see purely from evidence only, as a single string
- `rationale`: why this is the next best step

```json
{ "observation": "...", "action": "...", "rationale": "..." }
```

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — list directory contents
   {"action":"list_dir","path":"canon-utils","rationale":"Inspect the workspace before making assumptions."}

2. read_file — read a file before editing; output is always line-numbered ("42: code here")
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","rationale":"Read the file before editing it."}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120,"rationale":"Read the relevant section before editing it."}
   With "line":N the output starts at line N and shows up to 250 lines.
   ⚠ Always read a file before patching it. Never patch from memory.
   ⚠ read_file output is prefixed with line numbers ("42: code here"). Strip the "N: " prefix when
     writing patch lines — patch lines must contain ONLY the raw source text, never "42: code here".
     WRONG:  -42: fn old() {}   RIGHT:  -fn old() {}

3. apply_patch — create or update files
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Add File: path/to/new.rs\n+line one\n+line two\n*** End Patch","rationale":"Apply the concrete code change after reading the target context."}

   To UPDATE an existing file, each @@ hunk needs 3 unchanged context lines around the change:
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: src/lib.rs\n@@\n fn before_before() {}\n fn before() {}\n fn target() {\n-    old_body();\n+    new_body();\n }\n fn after() {}\n*** End Patch","rationale":"Update the file using exact surrounding context from the read."}

   Multiple hunks in one patch — each @@ is a separate location, each needs 3 context lines:
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: src/lib.rs\n@@\n fn aaa() {}\n fn bbb() {}\n fn ccc() {\n+    extra_line();\n }\n fn ddd() {}\n@@\n fn xxx() {}\n fn yyy() {}\n fn zzz() {\n-    old();\n+    new();\n }\n fn www() {}\n*** End Patch","rationale":"Apply multiple verified edits in one patch when the exact anchors are known."}

   WRONG — @@ with only 1 context line per hunk causes anchor-miss failures:
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: src/lib.rs\n@@\n fn ccc() {\n+    extra_line();\n@@\n fn zzz() {\n-    old();\n+    new();\n*** End Patch","rationale":"Example of a bad patch shape that will anchor-miss due to insufficient context."}

   Rules:
   - Every @@ hunk must have AT LEAST 3 unchanged context lines (space-prefixed) around the edit.
   - Never use @@ with only 1 context line — the patcher will fail to locate the anchor.
   - Context lines must be copied EXACTLY from read_file output (minus the "N: " prefix).
   - *** Add File for new files, *** Update File for existing files.
   - NEVER use absolute paths inside the patch string.

4. run_command — run shell commands for discovery or verification
   {"action":"run_command","cmd":"cargo check -p some-crate","cwd":"/workspace/ai_sandbox/canon","rationale":"Validate the target crate after a change."}
   {"action":"run_command","cmd":"rg -n 'fn foo' canon-utils/some-crate/src/","cwd":"/workspace/ai_sandbox/canon","rationale":"Search the codebase for the relevant symbol before editing."}

5. python — run Python analysis inside the workspace
   {"action":"python","code":"from pathlib import Path\nprint(len(list(Path('canon-utils').glob('**/*.rs'))))","cwd":"/workspace/ai_sandbox/canon","rationale":"Use Python for structured workspace analysis."}

6. done — declare the objective complete (triggers cargo build --workspace then cargo test --workspace)
   {"action":"done","reason":"brief evidence summary: files changed, commands run, outcomes, remaining uncertainty","rationale":"Execution work is complete and the verifier now has enough evidence to judge it."}
   ⚠ done is REJECTED if the build or any test fails — fix all errors first.

━━━ EVIDENCE HANDOFF ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

After completing each task or sub-task from your lane plan, do NOT update `PLANS/SPEC.md`, `PLANS/executor-a.md`, or `PLANS/executor-b.md` yourself.
Instead, use `done.reason` to report verifier-facing evidence:
- files changed
- commands run
- outcomes / failing checks
- remaining uncertainty or blockers

Read `PLANS/SPEC.md` and your assigned lane plan when needed for execution context, but leave planning-file mutation to planner.

Execution discipline:
- Prefer tasks explicitly marked ready / highest priority by the planner.
- Do not skip ahead to lower-priority or blocked tasks unless the current ready task is impossible and you have concrete evidence.
- Keep cycles short: complete at most 1-5 tasks before yielding control.
- If an apply_patch fails, read the exact file or line range before retrying.
- Do not repeat the same patch attempt without new evidence from read_file, run_command, or python.
- When touching routing, policy, observe, act, dispatch, or control-flow code, favor semantic-state authority over queue-truth heuristics.
- If a task conflicts with the canonical law above, execute the canonical law and report the conflict in `done.reason` so planner/verifier can update plan truth.

━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Emit exactly one action per turn.
- Always read a file before patching it.
- Use list_dir and read_file freely before assuming project state.
- Use run_command for cargo builds, tests, and shell discovery.
- Use python for structured analysis when shell pipelines are awkward.
- Never operate outside /workspace/ai_sandbox/canon.
- Never modify `PLANS/SPEC.md`, `PLANS/executor-a.md`, `PLANS/executor-b.md`, or `PLANS/diagnostics.md`.
- Never emit destructive commands (rm -rf, git reset --hard, git clean -f, etc.).
- Output format: exactly one JSON object in a ```json code block. No prose outside it.
"#;

const SYSTEM_INSTRUCTIONS_VERIFIER: &str = r#"You are the canon verifier agent.

Your job is to critically review executor evidence against the codebase and judge whether the implementation satisfies `PLANS/SPEC.md`.
Executor evidence and lane plans are hints only. The canonical truth is the codebase versus `PLANS/SPEC.md`.
Be skeptical — do not trust executor claims at face value.

Canonical law:
- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- `scheduler_len`, `planned_pending`, and other queue-like counters are not authoritative when semantic-state facts exist.
- A task is NOT verified if it leaves queue-driven routing in place where semantic-state routing was the intended fix.

You work inside the canon workspace at /workspace/ai_sandbox/canon.

Each turn you receive either:
  (a) the initial plan and instructions; or
  (b) the result of your last action.

You respond with exactly one action per turn, as a single JSON object wrapped in a `json` code block.
Available actions:
- `done`
- `list_dir`
- `read_file`
- `run_command`
- `python`
Every action MUST include:
- `observation`: what you can see purely from evidence only, as a single string
- `rationale`: why this is the next best step

```json
{ "observation": "...", "action": "...", "rationale": "..." }
```

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — explore directory contents
   {"action":"list_dir","path":"canon-utils","rationale":"Inspect the relevant area before verifying claims about it."}

2. read_file — read a source file; output is line-numbered ("42: code here")
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","rationale":"Read the source to verify whether the claimed change exists."}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120,"rationale":"Jump to the relevant section to verify the claimed change."}
   With "line":N the output starts at line N and shows up to 250 lines.
   ⚠ read_file output is prefixed with line numbers ("42: code here"). Strip the "N: " prefix when
     writing patch lines — patch lines must contain ONLY the raw source text, never "42: code here".
     WRONG:  -42: fn old() {}   RIGHT:  -fn old() {}

3. apply_patch — unavailable in verifier mode
   Do not use `apply_patch` as verifier.
   Judge the code against `PLANS/SPEC.md` and report findings in `done.reason`.

4. run_command — run build/test commands to verify correctness
   {"action":"run_command","cmd":"cargo check -p some-crate","cwd":"/workspace/ai_sandbox/canon","rationale":"Validate the crate implicated by the completed task."}
   {"action":"run_command","cmd":"cargo test --workspace","cwd":"/workspace/ai_sandbox/canon","rationale":"Verify the claimed completion does not break workspace tests."}
   {"action":"run_command","cmd":"rg -n 'fn foo'","cwd":"/workspace/ai_sandbox/canon","rationale":"Find the implementation or call sites mentioned by the completed task."}

5. python — run focused verification analysis
   {"action":"python","code":"from pathlib import Path\nprint(Path('PLANS/SPEC.md').exists())","cwd":"/workspace/ai_sandbox/canon","rationale":"Use Python when structured verification logic is easier than shell commands."}

6. done — declare verification complete — DO NOT say done if spec obligations are still unmet
   {"action":"done","reason":"{\"verified\":false,\"summary\":\"summary of findings: N tasks verified, M incorrect or missing\"}","rationale":"Verification is complete and the findings are summarized."}
   ⚠ done triggers cargo build --workspace then cargo test --workspace — fix any failures first.

━━━ VERIFICATION PROCESS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

For each executor claim:
1. Use the executor result summary plus `PLANS/SPEC.md` to derive the candidate obligations.
2. Read the relevant source files to confirm the described change exists.
3. Run cargo check or cargo test if the task involves code correctness.
4. Judge whether the code satisfies the spec.
5. Report verified or unverified status in `done.reason`.
6. For any routing/control-flow claim, verify whether decisions are derived from semantic state rather than queue-local heuristics.

━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Be critical and thorough — verify evidence, not just the claim.
- Do not mark anything verified unless you have read the actual code or seen passing tests.
- Do not modify `PLANS/SPEC.md`, `PLANS/executor-a.md`, `PLANS/executor-b.md`, or source files.
- Emit exactly one action per turn.
- Reject any claimed completion that still leaves `scheduler_len` or local queue mirrors acting as routing authority when `SemanticStateSummary` is available.
- When using `done`, the `reason` field must be a compact JSON object string with exactly:
  - `verified`: boolean
  - `summary`: string
- Output format: exactly one JSON object in a ```json code block. No prose outside it.
"#;

const SYSTEM_INSTRUCTIONS_PLANNER: &str = r#"You are the canon planner agent.

Your job is to read `PLANS/SPEC.md` and continuously derive executor lane plans.
You own priority, dependency ordering, task allocation, and the ready-work window for each executor.
On every cycle, re-evaluate the workspace and rewrite `PLANS/executor-a.md` and `PLANS/executor-b.md` so each executor only needs to perform the top 1-5 ready tasks.

Canonical law:
- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- `scheduler_len`, `planned_pending`, and similar counters are not root truth for routing.
- Prioritize work that migrates decision logic to semantic-state authority before local edge patches that preserve queue-truth.

You work inside the canon workspace at /workspace/ai_sandbox/canon. Use bash, rg, read_file, python, and diagnostics evidence to review the current project state before reorganizing the plan.

Each turn you receive either:
  (a) the initial plan; or
  (b) the result of your last action.

You respond with exactly one action per turn, as a single JSON object wrapped in a `json` code block.
Available actions:
- `done`
- `list_dir`
- `read_file`
- `apply_patch`
- `run_command`
- `python`
Every action MUST include:
- `observation`: what you can see purely from evidence only, as a single string
- `rationale`: why this is the next best step

```json
{ "observation": "...", "action": "...", "rationale": "..." }
```

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — explore directory contents
   {"action":"list_dir","path":"canon-utils","rationale":"Inspect the relevant code area before expanding tasks."}

2. read_file — read a source file; output is line-numbered ("42: code here")
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","rationale":"Read the source before deriving actionable plan steps."}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120,"rationale":"Read the relevant source section before deriving actionable plan steps."}
   ⚠ read_file output is prefixed with line numbers ("42: code here"). Strip the "N: " prefix when
     writing patch lines — patch lines must contain ONLY the raw source text, never "42: code here".
     WRONG:  -42: fn old() {}   RIGHT:  -fn old() {}

3. apply_patch — update `PLANS/executor-a.md` and `PLANS/executor-b.md` with derived lane plans, refreshed priorities, and concrete steps
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: PLANS/executor-a.md\n@@\n line_before_before\n line_before\n - [ ] task to expand\n+  1. sub-step one\n+  2. sub-step two\n line_after\n line_after_after\n*** End Patch","rationale":"Refresh a lane plan so ready work, dependencies, and priority are explicit."}

   Rules:
   - Every @@ hunk needs AT LEAST 3 unchanged context lines (space-prefixed) around the change.
   - NEVER chain multiple @@ blocks with only 1 context line each — every anchor needs 3 lines.
   - WRONG: @@\n - [ ] task\n+  1. sub-step\n@@\n - [ ] task2\n+  1. sub-step
   - RIGHT: @@\n prev_line\n prev_line2\n - [ ] task\n+  1. sub-step\n next_line\n next_line2

4. run_command — inspect the codebase
   {"action":"run_command","cmd":"rg -n 'fn foo'","cwd":"/workspace/ai_sandbox/canon","rationale":"Search for implementation details needed to expand the plan accurately."}

5. python — run structured planning analysis
   {"action":"python","code":"from pathlib import Path\nprint(sum(1 for _ in Path('canon-utils').glob('**/*.rs')))","cwd":"/workspace/ai_sandbox/canon","rationale":"Use Python to gather structured planning context from the workspace."}

6. done — declare the plan reorganization complete
   {"action":"done","reason":"reorganized plan into a DAG with refreshed priorities and a 1-5 task ready window","rationale":"Planning is complete and the plan is ready for the next executor cycle."}

━━━ PLANNING PROCESS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

On every planning cycle:
1. Read `PLANS/SPEC.md`, diagnostics, relevant source files, and recent workspace state to understand what changed.
2. Derive `PLANS/executor-a.md` and `PLANS/executor-b.md` from the spec.
3. Maintain a READY NOW window containing at most 1-5 executable tasks for each executor.
4. Move blocked work behind its dependencies instead of leaving it in the ready window.
5. Rewrite priorities whenever new evidence changes the critical path.
6. If queue-truth and semantic-state authority conflict, prioritize semantic-state authority and move queue-truth cleanup behind it as follow-on work.

━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Only modify `PLANS/executor-a.md` and `PLANS/executor-b.md` — never edit source files or `PLANS/SPEC.md`.
- The planner owns lane-task ordering, dependency structure, and ready-task selection.
- Prefer rewriting whole plan sections when needed so priority order stays globally coherent.
- Keep each executor's ready window small: 1-5 tasks maximum.
- Prefer root-cause tasks that remove queue-driven routing over local patches that merely suppress symptoms.
- Emit exactly one action per turn.
- Output format: exactly one JSON object in a ```json code block. No prose outside it.
"#;

const SYSTEM_INSTRUCTIONS_DIAGNOSTICS: &str = r#"You are the canon diagnostics agent.

Your job is to scan the canon project state, detect inconsistencies and failures, rank them by impact, and write concrete repair targets for the planner.

Canonical law:
- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- `scheduler_len`, `planned_pending`, and similar counters are not authoritative routing truth unless explicitly proven as derived mirrors.
- A high-impact failure exists whenever queue-local state still drives routing in places that should derive from semantic state.

You must inspect both:
- the project source tree under /workspace/ai_sandbox/canon
- the event log segments under /workspace/ai_sandbox/canon/state/event_log/event.tlog.d

Each turn you receive either:
  (a) the initial instruction; or
  (b) the result of your last action.

You respond with exactly one action per turn, as a single JSON object wrapped in a `json` code block.
Available actions:
- `done`
- `list_dir`
- `read_file`
- `apply_patch`
- `run_command`
- `python`
Every action MUST include:
- `observation`: what you can see purely from evidence only, as a single string
- `rationale`: why this is the next best step

```json
{ "observation": "...", "action": "...", "rationale": "..." }
```

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — inspect directories
   {"action":"list_dir","path":"state/event_log/event.tlog.d","rationale":"Inspect the available event-log segments before diagnosing failures."}
   {"action":"list_dir","path":"canon-utils","rationale":"Inspect the project layout before targeting diagnostics."}

2. read_file — read source files
   {"action":"read_file","path":"canon-utils/canon-route/src/policy.rs","line":1,"rationale":"Read a suspected source file to correlate code with observed failures."}

3. python — run Python analysis over project state and event logs
   {"action":"python","code":"from pathlib import Path\nroot = Path('/workspace/ai_sandbox/canon/state/event_log/event.tlog.d')\nfor path in sorted(root.glob('*.log')):\n    print(path.name, path.stat().st_size)","cwd":"/workspace/ai_sandbox/canon","rationale":"Analyze the event-source logs to find failure signals and inconsistencies."}

4. run_command — run bash for grep/build queries
   {"action":"run_command","cmd":"rg -n \"invariant|panic|TODO|unreachable!|assert!\" canon-utils state","cwd":"/workspace/ai_sandbox/canon","rationale":"Search the codebase and state for likely failure markers."}
   {"action":"run_command","cmd":"cargo check --workspace","cwd":"/workspace/ai_sandbox/canon","rationale":"Detect compiler-visible inconsistencies that belong in diagnostics."}

5. apply_patch — write the diagnostics report
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Add File: PLANS/diagnostics.md\n+# Diagnostics Report\n+...\n*** End Patch","rationale":"Write the ranked diagnostics report after collecting evidence from logs and code."}

6. done — declare diagnostics complete
   {"action":"done","reason":"diagnostics report written to PLANS/diagnostics.md","rationale":"Diagnostics is complete and the planner handoff has been recorded."}

━━━ DIAGNOSTICS PROCESS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Gather evidence from the event logs and the current codebase, then write PLANS/diagnostics.md with this structure:

# Diagnostics Report
## Inputs Scanned
- event log segments reviewed
- source areas reviewed
- commands run
## Ranked Failures
1. Impact: high|medium|low
   Signal: what is inconsistent or broken
   Evidence: exact files, commands, or event-log observations
   Repair Targets:
   - concrete file/module/function targets
   - specific invariants or behaviors to restore
## Planner Handoff
- ordered list of the highest-value repair targets
- blockers or missing evidence

Rules:
- Always inspect /workspace/ai_sandbox/canon/state/event_log/event.tlog.d on every invocation.
- Use the `python` action for structured analysis of event logs and project state.
- Only modify PLANS/diagnostics.md.
- Rank issues by impact on correctness, convergence, and repairability.
- Explicitly check whether routing/control-flow still depends on `scheduler_len`, `planned_pending`, or other local queue mirrors instead of `SemanticStateSummary`.
- Prioritize diagnostics that identify state-authority drift, synthetic dispatch bypasses, and queue-driven control decisions.
- Before trusting a trace file like /tmp/runtime.trace, confirm it was updated in the current cycle (mtime, size change, or fresh producer command).
- Treat empty `rg` / `grep` results on traces as ambiguous: no match, stale file, or incomplete write are all possible.
- Prefer latest event-log segments under state/event_log/event.tlog.d over ad-hoc temp traces when they disagree.
- Emit exactly one action per turn.
- Output format: exactly one JSON object in a ```json code block. No prose outside it.
"#;

// ── Action parsing ─────────────────────────────────────────────────────────────

fn parse_actions(raw: &str) -> Result<Vec<Value>> {
    if let Some(json_text) = extract_json_fence(raw) {
        return parse_json_action(json_text).with_context(|| "fenced json block was not a valid action object");
    }
    parse_json_action(raw.trim()).with_context(|| format!("response was not a JSON action object: {:?}", &raw.chars().take(200).collect::<String>()))
}

fn extract_json_fence(text: &str) -> Option<&str> {
    let start = text.find("```json").or_else(|| text.find("```JSON"))?;
    let after_newline = start + text[start..].find('\n')?;
    let rest = &text[after_newline + 1..];
    let end = rest.find("```")?;
    Some(rest[..end].trim())
}

fn parse_json_action(text: &str) -> Result<Vec<Value>> {
    if let Ok(obj) = serde_json::from_str::<Value>(text) {
        if obj.is_object() && obj.get("action").is_some() {
            return Ok(vec![obj]);
        }
    }
    if let Ok(arr) = serde_json::from_str::<Vec<Value>>(text) {
        if arr.len() == 1 && arr[0].is_object() && arr[0].get("action").is_some() {
            return Ok(arr);
        }
        bail!("expected exactly one action object, got array of len {}", arr.len());
    }
    bail!("not a JSON action object: {:?}", &text.chars().take(120).collect::<String>())
}

// ── Patch crate inference ──────────────────────────────────────────────────────

/// Extract the first file path touched by the patch (*** Update File: / *** Add File:).
fn patch_first_file(patch: &str) -> Option<&str> {
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("*** Update File:").or_else(|| line.strip_prefix("*** Add File:")) {
            let path = rest.trim();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    None
}

fn patch_targets<'a>(patch: &'a str) -> Vec<&'a str> {
    patch
        .lines()
        .filter_map(|line| line.strip_prefix("*** Update File:").or_else(|| line.strip_prefix("*** Add File:")))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .collect()
}

fn patch_scope_error(role: &str, patch: &str) -> Option<String> {
    let targets = patch_targets(patch);
    if targets.is_empty() {
        return None;
    }

    let touches_spec = targets.iter().any(|path| *path == SPEC_FILE);
    let touches_exec_a = targets.iter().any(|path| *path == EXECUTOR_A_PLAN_FILE);
    let touches_exec_b = targets.iter().any(|path| *path == EXECUTOR_B_PLAN_FILE);
    let touches_lane = touches_exec_a || touches_exec_b;
    let touches_diagnostics = targets.iter().any(|path| *path == DIAGNOSTICS_FILE);
    let touches_other = targets
        .iter()
        .any(|path| *path != SPEC_FILE && *path != EXECUTOR_A_PLAN_FILE && *path != EXECUTOR_B_PLAN_FILE && *path != DIAGNOSTICS_FILE);

    match role {
        "mini_agent" | "executor_a" | "executor_b" => {
            if touches_spec || touches_lane || touches_diagnostics {
                Some(
                    "Executor may not patch `PLANS/SPEC.md`, lane plans, or `PLANS/diagnostics.md`. Execute code/tests only and report evidence in `done.reason`."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "verifier" | "verifier_a" | "verifier_b" => {
            if touches_spec || touches_lane || touches_diagnostics || touches_other {
                Some(
                    "Verifier is read-only for `PLANS/SPEC.md`, lane plans, diagnostics, and source files. Judge the code against the spec and report via `done.reason`."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "planner" | "mini_planner" => {
            if touches_spec || touches_diagnostics || touches_other {
                Some(
                    "Planner may only patch `PLANS/executor-a.md` and `PLANS/executor-b.md` because planner derives lane plans from the spec."
                        .to_string(),
                )
            } else {
                None
            }
        }
        "diagnostics" => {
            if touches_spec || touches_lane || touches_other {
                Some(
                    "Diagnostics may only patch PLANS/diagnostics.md because diagnostics owns ranked failure reporting."
                        .to_string(),
                )
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Walk up from `file_path` (workspace-relative) to find the nearest Cargo.toml.
/// Returns the package name from that manifest, or None if not found.
fn infer_crate_for_patch(workspace: &Path, file_path: &str) -> Option<String> {
    let mut dir = workspace.join(file_path);
    dir.pop(); // start from parent of the file
    loop {
        let manifest = dir.join("Cargo.toml");
        if manifest.exists() {
            let text = std::fs::read_to_string(&manifest).ok()?;
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("name") {
                    let name = rest.trim().trim_start_matches('=').trim().trim_matches('"');
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
        if dir == workspace {
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

// ── Patch-anchor auto-read (mirrors harness_repair logic) ─────────────────────

const AUTO_READ_CONTEXT_BEFORE: usize = 20;
const AUTO_READ_CONTEXT_AFTER: usize = 40;

/// Extract the file path from an apply_patch anchor-miss error.
/// Matches: "Failed to find expected lines in PATH:\n..."
fn extract_anchor_fail_path(err_msg: &str) -> Option<String> {
    let prefix = "Failed to find expected lines in ";
    for line in err_msg.lines() {
        if let Some(rest) = line.strip_prefix(prefix) {
            let path = rest.trim_end_matches(':').trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

/// Parse the indented anchor lines out of the patch error message.
fn extract_expected_anchor_lines(err_msg: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut capture = false;
    for line in err_msg.lines() {
        if line.starts_with("Failed to find expected lines in ") {
            capture = true;
            continue;
        }
        if !capture {
            continue;
        }
        if line.trim().is_empty() {
            if !lines.is_empty() {
                break;
            }
            continue;
        }
        if line.starts_with("    ") || line.starts_with('\t') {
            lines.push(line.trim().to_string());
            continue;
        }
        if !lines.is_empty() {
            break;
        }
    }
    lines
}

fn patch_failure_guidance(path: Option<&str>, err_msg: &str) -> String {
    let mut hints = Vec::new();
    hints.push("Patch anchor miss: deleted/context lines must match the current file EXACTLY.".to_string());
    hints.push("Do not abbreviate deleted lines like `-1. Centralize d`; copy exact text from read_file output.".to_string());
    hints.push("Next step: emit `read_file` for the target file, then build a new patch with at least 3 unchanged context lines.".to_string());

    if let Some(file) = path {
        if file == DIAGNOSTICS_FILE || file.ends_with(".md") {
            hints.push("This is a prose/markdown file: prefer rewriting the whole section or the whole file instead of a tiny surgical hunk.".to_string());
            hints.push("For diagnostics.md, one full-file rewrite is usually more reliable than repeated partial patches.".to_string());
        }
    }

    let anchors = extract_expected_anchor_lines(err_msg);
    if !anchors.is_empty() {
        hints.push(format!("Failed anchor lines: {}", anchors.join(" | ")));
    }

    hints.join("\n")
}

/// Find the file region closest to the failed anchor and return a numbered excerpt.
fn extract_anchor_context_excerpt(full: &str, err_msg: &str) -> Option<(usize, usize, String)> {
    let anchor_lines = extract_expected_anchor_lines(err_msg);
    if anchor_lines.is_empty() {
        return None;
    }
    let file_lines: Vec<&str> = full.lines().collect();
    let mut best_idx: Option<usize> = None;
    for anchor in anchor_lines.iter().rev() {
        let needle = anchor.trim();
        if needle.len() < 8 {
            continue;
        }
        if let Some(idx) = file_lines.iter().position(|l| l.contains(needle)) {
            best_idx = Some(idx);
            break;
        }
    }
    let idx = best_idx?;
    let start_idx = idx.saturating_sub(AUTO_READ_CONTEXT_BEFORE);
    let end_idx = (idx + AUTO_READ_CONTEXT_AFTER + 1).min(file_lines.len());
    let start_line = start_idx + 1;
    let excerpt = file_lines[start_idx..end_idx].iter().enumerate().map(|(i, l)| format!("{}: {}", start_line + i, l)).collect::<Vec<_>>().join("\n");
    Some((start_line, end_idx, excerpt))
}

/// Auto-read the region near the failed anchor, falling back to the full file.
fn auto_read_for_patch_anchor(workspace: &Path, relative: &str, err_msg: &str) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let full = std::fs::read_to_string(&path).with_context(|| format!("auto-read failed: {}", path.display()))?;
    if let Some((start, end, excerpt)) = extract_anchor_context_excerpt(&full, err_msg) {
        return Ok(format!("Current content near likely match of failed anchor in {relative} (lines {start}-{end}):\n{excerpt}"));
    }
    // Fallback: first MAX_FULL_READ_LINES lines of the file.
    let text = full.lines().take(MAX_FULL_READ_LINES).enumerate().map(|(i, l)| format!("{}: {}", i + 1, l)).collect::<Vec<_>>().join("\n");
    Ok(format!("Current content of {relative}:\n{text}"))
}

// ── Action executors ───────────────────────────────────────────────────────────

fn exec_list_dir(workspace: &Path, relative: &str) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let mut entries = std::fs::read_dir(&path).with_context(|| format!("list_dir: {}", path.display()))?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
    entries.sort();
    Ok(entries.join("\n"))
}

fn exec_read_file(workspace: &Path, relative: &str, start_line: Option<usize>) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let full = std::fs::read_to_string(&path).with_context(|| format!("read_file: {}", path.display()))?;
    let lines: Vec<&str> = full.lines().collect();
    let total = lines.len();
    let (from, max_lines) = match start_line {
        Some(n) => (n.saturating_sub(1).min(total), 250),
        None => (0, MAX_FULL_READ_LINES),
    };
    let text = lines[from..].iter().take(max_lines).enumerate().map(|(i, l)| format!("{}: {}", from + i + 1, l)).collect::<Vec<_>>().join("\n");
    let shown = max_lines.min(total.saturating_sub(from));
    if total > from + shown {
        Ok(format!("{text}\n(file has {total} lines total; use \"line\":{} to read more)", from + shown + 1))
    } else {
        Ok(text)
    }
}

fn shell_tokens(cmd: &str) -> Vec<&str> {
    cmd.split(|c: char| c.is_whitespace() || matches!(c, '|' | '&' | ';' | '(' | ')' | '<' | '>'))
        .filter(|part| !part.is_empty())
        .collect()
}

fn contains_token_pair(cmd: &str, first: &str, second: &str) -> bool {
    let tokens = shell_tokens(cmd);
    tokens.windows(2).any(|window| window[0] == first && window[1] == second)
}

fn starts_direct_debug_binary(cmd: &str) -> bool {
    let first = shell_tokens(cmd).into_iter().next().unwrap_or("");
    first.starts_with("./target/debug/") || first.contains("/target/debug/")
}

fn looks_like_long_running_command(cmd: &str) -> bool {
    contains_token_pair(cmd, "cargo", "run")
        || contains_token_pair(cmd, "cargo", "watch")
        || starts_direct_debug_binary(cmd)
        || cmd.contains(" --tlog ")
        || cmd.contains("| tee")
}

fn exec_run_command(workspace: &Path, cmd: &str, cwd: &str) -> Result<(bool, String)> {
    let cwd_path = PathBuf::from(cwd);
    if !cwd_path.is_absolute() {
        bail!("run_command cwd must be absolute: {cwd}");
    }
    if !cwd_path.starts_with(workspace) {
        bail!("run_command cwd escapes workspace: {cwd}");
    }
    ensure_safe_command(cmd)?;
    // Hybrid execution model:
    // - long-running commands → spawn (non-blocking)
    // - short commands → capture output (blocking)

    let is_long_running = looks_like_long_running_command(cmd);

    if is_long_running {
        let child = Command::new("/bin/bash")
            .arg("-c")
            .arg(cmd)
            .current_dir(&cwd_path)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to spawn: {cmd}"))?;

        Ok((true, format!("spawned pid={}", child.id())))
    } else {
        let output = Command::new("/bin/bash").arg("-c").arg(cmd).current_dir(&cwd_path).output().with_context(|| format!("failed to spawn: {cmd}"))?;

        let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
        if !output.stderr.is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        if combined.trim().is_empty() && !output.status.success() {
            if cmd.contains("rg ") || cmd.contains("grep ") {
                combined = format!("no matches (exit={})", output.status.code().unwrap_or(-1));
                if cmd.contains("/tmp/runtime.trace") {
                    combined.push_str("\ntrace probe returned no matches; file may be stale, missing, or the pattern may not be present yet");
                }
            }
        }
        if cmd.contains("/tmp/runtime.trace") && (cmd.contains("rg ") || cmd.contains("grep ")) {
            let trace = PathBuf::from("/tmp/runtime.trace");
            match std::fs::metadata(&trace) {
                Ok(meta) => {
                    combined.push_str(&format!("\ntrace_path=/tmp/runtime.trace trace_size={}B", meta.len()));
                }
                Err(_) => {
                    combined.push_str("\ntrace_path=/tmp/runtime.trace trace_missing=true");
                }
            }
        }

        Ok((output.status.success(), combined))
    }
}

fn exec_python(workspace: &Path, code: &str, cwd: &str) -> Result<(bool, String)> {
    let cwd_path = PathBuf::from(cwd);
    if !cwd_path.is_absolute() {
        bail!("python cwd must be absolute: {cwd}");
    }
    if !cwd_path.starts_with(workspace) {
        bail!("python cwd escapes workspace: {cwd}");
    }
    let mut child = Command::new("python3")
        .arg("-")
        .current_dir(&cwd_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn python3 in {}", cwd_path.display()))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(code.as_bytes()).context("failed writing python stdin")?;
    }
    let output = child.wait_with_output().context("failed waiting for python3")?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok((output.status.success(), combined))
}

fn ensure_safe_command(cmd: &str) -> Result<()> {
    const BLOCKED: &[&str] = &["rm -rf", "git reset --hard", "git clean -f", "dd if=", "mkfs", "shred"];
    for needle in BLOCKED {
        if cmd.contains(needle) {
            bail!("blocked command: {cmd}");
        }
    }
    Ok(())
}

fn safe_join(workspace: &Path, relative: &str) -> Result<PathBuf> {
    let p = Path::new(relative);
    if p.is_absolute() {
        bail!("absolute paths not allowed: {relative}");
    }
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("path traversal not allowed: {relative}");
    }
    Ok(workspace.join(p))
}

fn truncate(s: &str, max: usize) -> &str {
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    &s[..end]
}

fn verifier_confirmed(reason: &str) -> bool {
    match serde_json::from_str::<Value>(reason) {
        Ok(v) => v.get("verified").and_then(|x| x.as_bool()).unwrap_or(false),
        Err(_) => false,
    }
}

fn diagnostics_python_reads_event_logs(action: &Value) -> bool {
    if action.get("action").and_then(|v| v.as_str()) != Some("python") {
        return false;
    }
    let code = action.get("code").and_then(|v| v.as_str()).unwrap_or("");
    code.contains("state/event_log/event.tlog.d")
}

fn action_rationale(action: &Value) -> Option<&str> {
    action.get("rationale").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

fn action_observation(action: &Value) -> Option<&str> {
    action.get("observation").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

fn default_rationale(kind: &str) -> &'static str {
    match kind {
        "list_dir" => "Inspect the workspace before making assumptions.",
        "read_file" => "Read the current file contents before acting on them.",
        "apply_patch" => "Apply the concrete change after gathering enough context.",
        "run_command" => "Run a command to inspect or verify the current state.",
        "python" => "Use Python for structured analysis that is awkward in shell.",
        "done" => "The required work appears complete and ready for final checks.",
        _ => "Take the next most justified step based on the available evidence.",
    }
}

fn normalize_action(action: &mut Value) -> Result<()> {
    let obj = action.as_object_mut().ok_or_else(|| anyhow!("action payload must be a JSON object"))?;
    let kind = obj.get("action").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("action missing 'action'"))?.to_string();
    if obj.get("rationale").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).is_none() {
        obj.insert("rationale".to_string(), Value::String(default_rationale(&kind).to_string()));
    }
    if kind == "done" && obj.get("reason").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).is_none() {
        return Err(anyhow!("done missing 'reason'"));
    }
    Ok(())
}

fn validate_action(action: &Value) -> Result<()> {
    let obj = action.as_object().ok_or_else(|| anyhow!("action payload must be a JSON object"))?;
    let kind = obj.get("action").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("action missing 'action'"))?;
    let observation = obj.get("observation").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("action missing non-empty 'observation'"))?;
    let rationale = obj.get("rationale").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("action missing non-empty 'rationale'"))?;
    let _ = (observation, rationale);
    if kind == "done" {
        obj.get("reason").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("done missing non-empty 'reason'"))?;
    }
    Ok(())
}

fn is_explicit_idle_action(action: &Value) -> bool {
    if action.get("action").and_then(|v| v.as_str()) != Some("run_command") {
        return false;
    }
    let cmd = action.get("cmd").and_then(|v| v.as_str()).unwrap_or("").trim();
    matches!(cmd, "echo idle" | "echo \"idle\"" | "true" | ":")
}

fn action_command_summary(action: &Value) -> String {
    let kind = action.get("action").and_then(|v| v.as_str()).unwrap_or("unknown");
    match kind {
        "run_command" => action.get("cmd").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        "python" => {
            let code = action.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let first = code.lines().next().unwrap_or("");
            format!("python: {}", truncate(first, 160))
        }
        "read_file" => {
            let path = action.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let line = action.get("line").and_then(|v| v.as_u64());
            match line {
                Some(n) => format!("read_file {}:{}", path, n),
                None => format!("read_file {}", path),
            }
        }
        "list_dir" => format!("list_dir {}", action.get("path").and_then(|v| v.as_str()).unwrap_or("")),
        "apply_patch" => {
            let patch = action.get("patch").and_then(|v| v.as_str()).unwrap_or("");
            patch_first_file(patch).map(|path| format!("apply_patch {}", path)).unwrap_or_else(|| "apply_patch".to_string())
        }
        "done" => format!("done {}", action.get("reason").and_then(|v| v.as_str()).unwrap_or("")),
        _ => kind.to_string(),
    }
}

fn append_action_log(role: &str, action: &Value) -> Result<()> {
    let observation = action_observation(action).unwrap_or("");
    let rationale = action_rationale(action).unwrap_or("");
    let record = json!({
        "ts_ms": canon_llm::endpoint_worker::tab_manager_now_ms(),
        "agent_type": role,
        "observation": observation,
        "action": action.get("action").and_then(|v| v.as_str()).unwrap_or("unknown"),
        "command_used": action_command_summary(action),
        "rationale": rationale,
    });
    let path = PathBuf::from(ACTION_LOG_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&record)?).with_context(|| format!("failed to append {}", path.display()))?;
    Ok(())
}

// ── Agent loop ─────────────────────────────────────────────────────────────────

/// Run one agent role until it calls `done` or exhausts MAX_STEPS.
/// Returns the done reason on success, or an error on hard failure.
/// `check_on_done`: if true, run cargo build + test before accepting done.
async fn run_agent(
    role: &str, system_instructions: &str, initial_prompt: String, endpoint: &LlmEndpoint, bridge: &WsBridge, workspace: &Path, _config: &CapabilityConfig, tabs: &TabManagerHandle, submit_only: bool,
    check_on_done: bool,
) -> Result<String> {
    let mut step = 0usize;
    let mut last_result: Option<String> = None;
    let mut diagnostics_eventlog_python_done = false;
    let mut idle_streak = 0usize;

    loop {
        if step >= MAX_STEPS {
            bail!("[{role}] exhausted {MAX_STEPS} steps without completing");
        }

        let (role_schema, prompt) = if step == 0 {
            (system_instructions.to_string(), initial_prompt.clone())
        } else {
            let result = last_result.as_deref().unwrap_or("");
            (String::new(), format!("Action result:\n{}\n\nEmit exactly one action.", truncate(result, MAX_SNIPPET)))
        };

        eprintln!("[{role}] step={} prompt_bytes={}", step + 1, prompt.len());

        let raw = match llm_worker_send_request(
            bridge,
            &endpoint.id,
            &endpoint.url,
            endpoint.stateful,
            &prompt,
            &role_schema,
            None,
            None,
            false,
            true,
            role,
            tabs,
            endpoint.max_tabs,
            submit_only,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[{role}] step={} llm_error: {e}", step + 1);
                last_result = Some(format!("LLM error: {e}\nReturn exactly one action as a single JSON object in a ```json code block."));
                step += 1;
                continue;
            }
        };

        eprintln!("[{role}] step={} response_bytes={}", step + 1, raw.len());

        let actions = match parse_actions(&raw) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[{role}] step={} parse_error: {e}", step + 1);
                last_result = Some(format!("Parse error: {e}\nReturn exactly one action as a single JSON object in a ```json code block. No prose outside it."));
                step += 1;
                continue;
            }
        };

        if actions.len() != 1 {
            let msg = format!("Got {} actions — emit exactly one action per turn.", actions.len());
            eprintln!("[{role}] step={} {msg}", step + 1);
            last_result = Some(msg);
            step += 1;
            continue;
        }

        let mut action = actions[0].clone();
        if let Err(e) = normalize_action(&mut action) {
            last_result = Some(format!("Invalid action: {e}\nReturn exactly one action with a non-empty `observation`, a non-empty `rationale`, and any required fields."));
            step += 1;
            continue;
        }
        if let Err(e) = validate_action(&action) {
            last_result = Some(format!("Invalid action: {e}\nReturn exactly one action with a non-empty `observation`, a non-empty `rationale`, and any required fields."));
            step += 1;
            continue;
        }
        let kind = action.get("action").and_then(|v| v.as_str()).unwrap_or("unknown");
        eprintln!("[{role}] step={} action={kind}", step + 1);

        if let Err(e) = append_action_log(role, &action) {
            eprintln!("[{role}] step={} action_log_error: {e}", step + 1);
        }

        if role == "diagnostics" && !diagnostics_eventlog_python_done {
            if diagnostics_python_reads_event_logs(&action) {
                diagnostics_eventlog_python_done = true;
            } else if step == 0 {
                last_result = Some(
                    "Diagnostics must begin with a `python` action that analyzes /workspace/ai_sandbox/canon/state/event_log/event.tlog.d to diagnose problems, detect inconsistencies, and extract concrete failure signals."
                        .to_string(),
                );
                step += 1;
                continue;
            } else if matches!(kind, "apply_patch" | "done") {
                last_result = Some(
                    "Before writing diagnostics or finishing, run a `python` action that analyzes /workspace/ai_sandbox/canon/state/event_log/event.tlog.d to find errors, inconsistencies, invariant violations, repeated failure patterns, and concrete repair targets. Diagnostics is for finding what is broken."
                        .to_string(),
                );
                step += 1;
                continue;
            }
        }

        if is_explicit_idle_action(&action) {
            idle_streak += 1;
            if idle_streak >= 3 {
                bail!("[{role}] stuck: no progress in 3 steps (repeated explicit idle commands)");
            }
        } else {
            idle_streak = 0;
        }

        let step_result: Result<(bool, String)> = (|| match kind {
            "done" => {
                let reason = action.get("reason").and_then(|v| v.as_str()).unwrap_or("complete");
                if !check_on_done {
                    return Ok((true, reason.to_string()));
                }
                eprintln!("[{role}] step={} done — running cargo build --workspace", step + 1);
                let (build_ok, build_out) = exec_run_command(workspace, "cargo build --workspace", WORKSPACE).unwrap_or_else(|e| (false, e.to_string()));
                if !build_ok {
                    eprintln!("[{role}] step={} cargo build failed — rejecting done", step + 1);
                    return Ok((false, format!("done rejected: cargo build --workspace failed.\n\n{}", truncate(&build_out, MAX_SNIPPET))));
                }
                eprintln!("[{role}] step={} cargo build ok — running cargo test --workspace", step + 1);
                let (test_ok, test_out) = exec_run_command(workspace, "cargo test --workspace", WORKSPACE).unwrap_or_else(|e| (false, e.to_string()));
                if test_ok {
                    eprintln!("[{role}] step={} cargo test ok — accepting done", step + 1);
                    Ok((true, reason.to_string()))
                } else {
                    eprintln!("[{role}] step={} cargo test failed — rejecting done", step + 1);
                    Ok((false, format!("done rejected: cargo test --workspace failed.\n\n{}", truncate(&test_out, MAX_SNIPPET))))
                }
            }
            "list_dir" => {
                let path = action.get("path").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("list_dir missing 'path'"))?;
                let out = exec_list_dir(workspace, path)?;
                Ok((false, format!("list_dir {path}:\n{out}")))
            }
            "read_file" => {
                let path = action.get("path").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("read_file missing 'path'"))?;
                let line = action.get("line").and_then(|v| v.as_u64()).map(|n| n as usize);
                let out = exec_read_file(workspace, path, line)?;
                eprintln!("[{role}] step={} read_file path={path} bytes={}", step + 1, out.len());
                Ok((false, format!("read_file {path}:\n{out}")))
            }
            "apply_patch" => {
                let patch = action.get("patch").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("apply_patch missing 'patch'"))?;
                if let Some(msg) = patch_scope_error(role, patch) {
                    Ok((false, msg))
                } else {
                    match apply_patch(patch, workspace) {
                        Ok(_) => {
                            eprintln!("[{role}] step={} apply_patch ok", step + 1);
                            let check_result = patch_first_file(patch).and_then(|f| infer_crate_for_patch(workspace, f)).map(|krate| {
                                eprintln!("[{role}] step={} cargo check -p {krate}", step + 1);
                                exec_run_command(workspace, &format!("cargo check -p {krate}"), WORKSPACE).unwrap_or_else(|e| (false, e.to_string()))
                            });
                            match check_result {
                                Some((ok, out)) => {
                                    let label = if ok { "cargo check ok" } else { "cargo check failed" };
                                    eprintln!("[{role}] step={} {label}", step + 1);
                                    Ok((false, format!("apply_patch ok\n\n{label}:\n{}", truncate(&out, MAX_SNIPPET))))
                                }
                                None => Ok((false, "apply_patch ok".to_string())),
                            }
                        }
                        Err(e) => {
                            let err_str = e.to_string();
                            eprintln!("[{role}] step={} apply_patch failed: {err_str}", step + 1);
                            let read_path = extract_anchor_fail_path(&err_str).or_else(|| patch_first_file(patch).map(|s| s.to_string()));
                            let guidance = patch_failure_guidance(read_path.as_deref(), &err_str);
                            let mut msg = format!("apply_patch failed: {err_str}\n\n{guidance}");
                            if let Some(fp) = read_path {
                                if let Ok(content) = auto_read_for_patch_anchor(workspace, &fp, &err_str) {
                                    eprintln!("[{role}] step={} auto_read path={fp}", step + 1);
                                    msg = format!("apply_patch failed: {err_str}\n\n{guidance}\n\n{content}");
                                }
                            }
                            Ok((false, msg))
                        }
                    }
                }
            }
            "run_command" => {
                let cmd = action.get("cmd").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("run_command missing 'cmd'"))?;
                let cwd = action.get("cwd").and_then(|v| v.as_str()).unwrap_or(WORKSPACE);
                eprintln!("[{role}] step={} run_command cmd={cmd}", step + 1);
                let (success, out) = exec_run_command(workspace, cmd, cwd)?;
                let label = if success { "run_command ok" } else { "run_command failed" };
                eprintln!("[{role}] step={} {label} output_bytes={}", step + 1, out.len());
                Ok((false, format!("{label}:\n{}", truncate(&out, MAX_SNIPPET))))
            }
            "python" => {
                let code = action.get("code").and_then(|v| v.as_str()).ok_or_else(|| anyhow!("python missing 'code'"))?;
                let cwd = action.get("cwd").and_then(|v| v.as_str()).unwrap_or(WORKSPACE);
                eprintln!("[{role}] step={} python bytes={}", step + 1, code.len());
                let (success, out) = exec_python(workspace, code, cwd)?;
                let label = if success { "python ok" } else { "python failed" };
                eprintln!("[{role}] step={} {label} output_bytes={}", step + 1, out.len());
                Ok((false, format!("{label}:\n{}", truncate(&out, MAX_SNIPPET))))
            }
            other => Ok((false, format!("unsupported action '{other}' — use list_dir, read_file, apply_patch, run_command, python, or done"))),
        })();

        match step_result {
            Ok((true, reason)) => {
                eprintln!("[{role}] done: {reason}");
                return Ok(reason);
            }
            Ok((false, out)) => {
                last_result = Some(out);
            }
            Err(e) => {
                eprintln!("[{role}] step={} error: {e}", step + 1);
                last_result = Some(format!("Error executing action: {e}"));
            }
        }
        step += 1;
    }
}

fn find_endpoint<'a>(config: &'a CapabilityConfig, role: &str) -> Result<&'a LlmEndpoint> {
    config.llm_endpoints.iter().find(|e| e.role.as_deref() == Some(role)).ok_or_else(|| anyhow!("no endpoint with role '{role}' in capability_config.toml"))
}

// ── Main ───────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let orchestrate = args.iter().any(|a| a == "--orchestrate");
    let start_role = args.windows(2).find(|w| w[0] == "--start").map(|w| w[1].as_str()).unwrap_or("executor");
    if !matches!(start_role, "executor" | "verifier" | "planner" | "diagnostics") {
        bail!("invalid --start value: {start_role} (expected executor|verifier|planner|diagnostics)");
    }
    let is_verifier = !orchestrate && args.iter().any(|a| a == "--verifier");
    let is_planner = !orchestrate && args.iter().any(|a| a == "--planner");
    let is_diagnostics = !orchestrate && args.iter().any(|a| a == "--diagnostics");
    let ws_port: u16 = args.windows(2).find(|w| w[0] == "--port").and_then(|w| w[1].parse().ok()).unwrap_or(WS_PORT_DEFAULT);

    let workspace = PathBuf::from(WORKSPACE);
    let spec_path = workspace.join(SPEC_FILE);
    let exec_a_plan_path = workspace.join(EXECUTOR_A_PLAN_FILE);
    let exec_b_plan_path = workspace.join(EXECUTOR_B_PLAN_FILE);

    let config = CapabilityConfig::snapshot_store_load().context("failed to load capability_config.toml")?;

    let ws_addr: std::net::SocketAddr = format!("127.0.0.1:{ws_port}").parse()?;
    let bridge = ws_server::spawn(ws_addr, config.response_timeout_secs, Arc::new(OnceLock::new()));
    eprintln!("[canon-mini-agent] waiting for Chrome extension on ws://127.0.0.1:{ws_port}");
    bridge.wait_for_connection().await;
    eprintln!("[canon-mini-agent] Chrome extension connected");

    let tabs = llm_worker_new_tabs();
    let tabs_exec_a = llm_worker_new_tabs();
    let tabs_exec_b = llm_worker_new_tabs();

    if orchestrate {
        const MAX_CYCLES: usize = 10;
        eprintln!("[orchestrate] start_role={start_role}");
        let mut last_verifier_summary = String::new();
        for cycle in 0..MAX_CYCLES {
            eprintln!("[orchestrate] ── cycle {} ──────────────────────────────", cycle + 1);

            {
                let ep = find_endpoint(&config, "diagnostics")?.clone();
                let prompt = format!(
                    "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\n\
Always inspect state/event_log/event.tlog.d and the relevant canon system files.\n\
Prioritize canon-route, canon-loop, canon-runtime, and canon-mini-agent when control flow or prompt contracts are implicated.\n\n\
Latest verifier summary:\n{}\n\n\
Use {SPEC_FILE} as the canonical contract, not lane plans.\n\
Infer failures from code, logs, runtime state, and verifier findings.\n\
Focus on route/control-flow correctness, event successor discharge, duplicate fanout, scheduler-state drift, and prompt-shell mismatches.\n\n\
Write a ranked diagnostics report to {DIAGNOSTICS_FILE}. Emit exactly one action to begin.",
                    if last_verifier_summary.is_empty() { "(none yet)" } else { &last_verifier_summary }
                );
                eprintln!("[orchestrate] cycle={} starting diagnostics", cycle + 1);
            let _ = run_agent("diagnostics", SYSTEM_INSTRUCTIONS_DIAGNOSTICS, prompt, &ep, &bridge, &workspace, &config, &tabs, false, false).await?;
            }

            let diagnostics = std::fs::read_to_string(workspace.join(DIAGNOSTICS_FILE)).unwrap_or_default();
            let spec = std::fs::read_to_string(&spec_path).with_context(|| format!("failed to read {SPEC_FILE}"))?;
            let planner_ep = find_endpoint(&config, "mini_planner")?.clone();
            eprintln!("[orchestrate] starting planner");
            let planner_prompt = format!(
                "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{spec}\n\nDiagnostics report (from {DIAGNOSTICS_FILE}):\n{diagnostics}\n\nCanonical law:\n- SemanticStateSummary is the single source of truth for routing.\n- scheduler_len / planned_pending are not routing authority.\n- Prioritize migration to state-authority before edge patches.\n- Derive lane plans from the spec; do not change spec truth.\n\nCreate or refresh {EXECUTOR_A_PLAN_FILE} and {EXECUTOR_B_PLAN_FILE}. Emit exactly one action to begin."
            );
            let _plan_result = run_agent("planner", SYSTEM_INSTRUCTIONS_PLANNER, planner_prompt, &planner_ep, &bridge, &workspace, &config, &tabs, false, false).await?;

            let exec_a_plan = std::fs::read_to_string(&exec_a_plan_path).with_context(|| format!("failed to read {EXECUTOR_A_PLAN_FILE}"))?;
            let exec_a_ep = find_endpoint(&config, "mini_agent")?.clone();
            let exec_a_prompt = format!(
                "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{spec}\n\nAssigned lane plan (from {EXECUTOR_A_PLAN_FILE}):\n{exec_a_plan}\n\nYou are executor A. Work only on the highest-priority READY items from your lane plan. Do not modify spec, lane plans, or diagnostics. Use `done.reason` to report evidence for verifier review. Emit exactly one action to begin."
            );

            let exec_b_plan = std::fs::read_to_string(&exec_b_plan_path).with_context(|| format!("failed to read {EXECUTOR_B_PLAN_FILE}"))?;
            let exec_b_ep = find_endpoint(&config, "executor_b")?.clone();
            let exec_b_prompt = format!(
                "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{spec}\n\nAssigned lane plan (from {EXECUTOR_B_PLAN_FILE}):\n{exec_b_plan}\n\nYou are executor B. Work only on the highest-priority READY items from your lane plan and avoid duplicating executor A's lane. Do not modify spec, lane plans, or diagnostics. Use `done.reason` to report evidence for verifier review. Emit exactly one action to begin."
            );

            let tabs_verify_a = llm_worker_new_tabs();
            let tabs_verify_b = llm_worker_new_tabs();
            let verifier_ep_a = find_endpoint(&config, "verifier")?.clone();
            let verifier_ep_b = find_endpoint(&config, "verifier")?.clone();
            const MAX_LANE_PIPELINE_ROUNDS: usize = 4;

            eprintln!("[orchestrate] starting executor/verifier lanes concurrently");
            let lane_a = async {
                let mut latest_verify_a =
                    "{\"verified\":false,\"summary\":\"lane_a not yet verified\"}".to_string();
                let mut next_exec_a_prompt = exec_a_prompt.clone();

                for lane_round in 0..MAX_LANE_PIPELINE_ROUNDS {
                    eprintln!("[orchestrate] lane_a round={}", lane_round + 1);
                    let exec_a_result = run_agent(
                        "executor_a",
                        SYSTEM_INSTRUCTIONS_EXECUTOR,
                        next_exec_a_prompt.clone(),
                        &exec_a_ep,
                        &bridge,
                        &workspace,
                        &config,
                        &tabs_exec_a,
                        false,
                        false,
                    )
                    .await?;

                    let verify_spec_a =
                        std::fs::read_to_string(&spec_path).with_context(|| format!("failed to read {SPEC_FILE}"))?;
                    let verifier_prompt_a = format!(
                        "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nExecutor lane: A\nExecutor result summary:\n{exec_a_result}\n\nCanonical spec (from {SPEC_FILE}):\n{verify_spec_a}\n\nVerify whether the current code satisfies the spec. Executor evidence is only a hint. Emit exactly one action to begin."
                    );
                    let verify_a_result = run_agent(
                        "verifier_a",
                        SYSTEM_INSTRUCTIONS_VERIFIER,
                        verifier_prompt_a,
                        &verifier_ep_a,
                        &bridge,
                        &workspace,
                        &config,
                        &tabs_verify_a,
                        false,
                        false,
                    )
                    .await?;

                    if verifier_confirmed(&verify_a_result) {
                        return Ok::<String, anyhow::Error>(verify_a_result);
                    }

                    latest_verify_a = verify_a_result.clone();
                    let refreshed_exec_a_plan = std::fs::read_to_string(&exec_a_plan_path)
                        .with_context(|| format!("failed to read {EXECUTOR_A_PLAN_FILE}"))?;
                    next_exec_a_prompt = format!(
                        "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{spec}\n\nAssigned lane plan (from {EXECUTOR_A_PLAN_FILE}):\n{refreshed_exec_a_plan}\n\nLatest verifier result for lane A:\n{latest_verify_a}\n\nContinue executor A immediately on the next highest-priority READY work. Address verifier findings first. Do not modify spec, lane plans, or diagnostics. Use `done.reason` to report evidence for verifier review. Emit exactly one action to begin."
                    );
                }

                Ok::<String, anyhow::Error>(latest_verify_a)
            };

            let lane_b = async {
                let mut latest_verify_b =
                    "{\"verified\":false,\"summary\":\"lane_b not yet verified\"}".to_string();
                let mut next_exec_b_prompt = exec_b_prompt.clone();

                for lane_round in 0..MAX_LANE_PIPELINE_ROUNDS {
                    eprintln!("[orchestrate] lane_b round={}", lane_round + 1);
                    let exec_b_result = run_agent(
                        "executor_b",
                        SYSTEM_INSTRUCTIONS_EXECUTOR,
                        next_exec_b_prompt.clone(),
                        &exec_b_ep,
                        &bridge,
                        &workspace,
                        &config,
                        &tabs_exec_b,
                        false,
                        false,
                    )
                    .await?;

                    let verify_spec_b =
                        std::fs::read_to_string(&spec_path).with_context(|| format!("failed to read {SPEC_FILE}"))?;
                    let verifier_prompt_b = format!(
                        "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nExecutor lane: B\nExecutor result summary:\n{exec_b_result}\n\nCanonical spec (from {SPEC_FILE}):\n{verify_spec_b}\n\nVerify whether the current code satisfies the spec. Executor evidence is only a hint. Emit exactly one action to begin."
                    );
                    let verify_b_result = run_agent(
                        "verifier_b",
                        SYSTEM_INSTRUCTIONS_VERIFIER,
                        verifier_prompt_b,
                        &verifier_ep_b,
                        &bridge,
                        &workspace,
                        &config,
                        &tabs_verify_b,
                        false,
                        false,
                    )
                    .await?;

                    if verifier_confirmed(&verify_b_result) {
                        return Ok::<String, anyhow::Error>(verify_b_result);
                    }

                    latest_verify_b = verify_b_result.clone();
                    let refreshed_exec_b_plan = std::fs::read_to_string(&exec_b_plan_path)
                        .with_context(|| format!("failed to read {EXECUTOR_B_PLAN_FILE}"))?;
                    next_exec_b_prompt = format!(
                        "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{spec}\n\nAssigned lane plan (from {EXECUTOR_B_PLAN_FILE}):\n{refreshed_exec_b_plan}\n\nLatest verifier result for lane B:\n{latest_verify_b}\n\nContinue executor B immediately on the next highest-priority READY work. Address verifier findings first and avoid duplicating lane A. Do not modify spec, lane plans, or diagnostics. Use `done.reason` to report evidence for verifier review. Emit exactly one action to begin."
                    );
                }

                Ok::<String, anyhow::Error>(latest_verify_b)
            };

            let (verify_a_result, verify_b_result) = tokio::join!(lane_a, lane_b);
            let verify_a_result = verify_a_result?;
            let verify_b_result = verify_b_result?;
            last_verifier_summary =
                format!("lane_a={verify_a_result}\nlane_b={verify_b_result}");

            let verify_result = if verifier_confirmed(&verify_a_result) && verifier_confirmed(&verify_b_result) {
                "{\"verified\":true,\"summary\":\"both verifier lanes confirmed completion\"}".to_string()
            } else {
                format!(
                    "{{\"verified\":false,\"summary\":\"lane_a={} | lane_b={}\"}}",
                    verify_a_result.replace('"', "'"),
                    verify_b_result.replace('"', "'")
                )
            };

            if verifier_confirmed(&verify_result) {
                eprintln!("[orchestrate] verifier confirms completion — done after {} cycle(s)", cycle + 1);
                println!("orchestrate: converged after {} cycle(s)", cycle + 1);
                return Ok(());
            }

            eprintln!("[orchestrate] verifier detected incomplete work — continuing");
        }
        bail!("orchestrate: did not converge after {MAX_CYCLES} cycles");
    } else {
        // Single-role mode
        let (role, instructions) = if is_verifier {
            ("verifier", SYSTEM_INSTRUCTIONS_VERIFIER)
        } else if is_diagnostics {
            ("diagnostics", SYSTEM_INSTRUCTIONS_DIAGNOSTICS)
        } else if is_planner {
            ("mini_planner", SYSTEM_INSTRUCTIONS_PLANNER)
        } else {
            ("mini_agent", SYSTEM_INSTRUCTIONS_EXECUTOR)
        };

        let primary_input_path = if is_verifier || is_planner {
            &spec_path
        } else {
            &exec_a_plan_path
        };
        let primary_input_name = if is_verifier || is_planner {
            SPEC_FILE
        } else {
            EXECUTOR_A_PLAN_FILE
        };
        let primary_input = std::fs::read_to_string(primary_input_path).with_context(|| format!("failed to read {primary_input_name}"))?;
        if primary_input.trim().is_empty() {
            bail!("input file is empty — write content into {primary_input_name} before running");
        }
        eprintln!("[canon-mini-agent] role={role} input loaded ({} bytes)", primary_input.len());

        let endpoint = find_endpoint(&config, role)?.clone();
        eprintln!("[canon-mini-agent] endpoint id={} url={}", endpoint.id, endpoint.url);

        let initial_prompt = if is_verifier {
            format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{primary_input}\n\nVerify whether the current code satisfies the spec. Emit exactly one action to begin.")
        } else if is_diagnostics {
            format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nAlways inspect state/event_log/event.tlog.d and the relevant canon system files.\nPrioritize canon-route, canon-loop, canon-runtime, canon-semantic-state, and canon-mini-agent when control flow or prompt contracts are implicated.\nLatest verifier summary:\n(none yet)\n\nUse {SPEC_FILE} as the contract, not lane plans.\nInfer failures from code, logs, runtime state, and verifier findings.\nCanonical law:\n- SemanticStateSummary is the single source of truth for routing.\n- scheduler_len / planned_pending are not routing authority.\nFocus on route/control-flow correctness, event successor discharge, duplicate fanout, state-authority drift, queue-driven routing, synthetic dispatch bypasses, and prompt-shell mismatches.\n\nWrite a ranked diagnostics report to {DIAGNOSTICS_FILE}. Emit exactly one action to begin.")
        } else if is_planner {
            let diagnostics = std::fs::read_to_string(workspace.join(DIAGNOSTICS_FILE)).unwrap_or_default();
            format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{primary_input}\n\nDiagnostics report (from {DIAGNOSTICS_FILE}):\n{diagnostics}\n\nCanonical law:\n- SemanticStateSummary is the single source of truth for routing.\n- scheduler_len / planned_pending are not routing authority.\n- Prioritize migration to state-authority before edge patches.\n\nDerive {EXECUTOR_A_PLAN_FILE} and {EXECUTOR_B_PLAN_FILE} from the spec. Emit exactly one action to begin.")
        } else {
            let spec = std::fs::read_to_string(&spec_path).with_context(|| format!("failed to read {SPEC_FILE}"))?;
            format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical spec (from {SPEC_FILE}):\n{spec}\n\nAssigned lane plan (from {EXECUTOR_A_PLAN_FILE}):\n{primary_input}\n\nDo not modify spec, lane plans, or diagnostics. Use `done.reason` to report evidence for verifier review. Emit exactly one action to begin.")
        };

        let submit_only = role == "mini_agent" || role == "executor_a" || role == "executor_b";
        let reason = run_agent(
            role,
            instructions,
            initial_prompt,
            &endpoint,
            &bridge,
            &workspace,
            &config,
            &tabs,
            submit_only,
            false,
        ).await?;
        println!("done: {reason}");
        Ok(())
    }
}

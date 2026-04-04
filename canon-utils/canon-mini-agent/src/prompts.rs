use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::{
    diagnostics_file, INVARIANTS_FILE, WORKSPACE, SPEC_FILE, OBJECTIVES_FILE, MASTER_PLAN_FILE, VIOLATIONS_FILE, MAX_SNIPPET,
};

pub(crate) fn truncate(s: &str, max: usize) -> &str {
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    &s[..end]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentPromptKind {
    Executor,
    Verifier,
    Planner,
    Diagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToolPromptKind {
    ListDir,
    ReadFile,
    ApplyPatch,
    RunCommand,
    Python,
    Done,
}

fn available_actions(kind: AgentPromptKind) -> &'static [&'static str] {
    match kind {
        AgentPromptKind::Verifier => &["done", "list_dir", "read_file", "apply_patch", "run_command", "python"],
        AgentPromptKind::Executor | AgentPromptKind::Planner | AgentPromptKind::Diagnostics => {
            &["done", "list_dir", "read_file", "apply_patch", "run_command", "python"]
        }
    }
}

fn tool_order(kind: AgentPromptKind) -> &'static [ToolPromptKind] {
    match kind {
        AgentPromptKind::Diagnostics => &[
            ToolPromptKind::ListDir,
            ToolPromptKind::ReadFile,
            ToolPromptKind::Python,
            ToolPromptKind::RunCommand,
            ToolPromptKind::ApplyPatch,
            ToolPromptKind::Done,
        ],
        AgentPromptKind::Verifier => &[
            ToolPromptKind::ListDir,
            ToolPromptKind::ReadFile,
            ToolPromptKind::ApplyPatch,
            ToolPromptKind::RunCommand,
            ToolPromptKind::Python,
            ToolPromptKind::Done,
        ],
        AgentPromptKind::Executor | AgentPromptKind::Planner => &[
            ToolPromptKind::ListDir,
            ToolPromptKind::ReadFile,
            ToolPromptKind::ApplyPatch,
            ToolPromptKind::RunCommand,
            ToolPromptKind::Python,
            ToolPromptKind::Done,
        ],
    }
}

fn tool_title(kind: AgentPromptKind, tool: ToolPromptKind) -> &'static str {
    match (kind, tool) {
        (_, ToolPromptKind::ListDir) => "list_dir — inspect directory contents",
        (_, ToolPromptKind::ReadFile) => "read_file — read a file; output is line-numbered (\"42: code here\")",
        (AgentPromptKind::Verifier, ToolPromptKind::ApplyPatch) => {
            "apply_patch — write `VIOLATIONS.md`"
        }
        (AgentPromptKind::Planner, ToolPromptKind::ApplyPatch) => {
            "apply_patch — update `PLAN.md` and lane plans under `PLANS/executor-<id>.md`"
        }
        (AgentPromptKind::Diagnostics, ToolPromptKind::ApplyPatch) => {
            "apply_patch — write the diagnostics report"
        }
        (_, ToolPromptKind::ApplyPatch) => "apply_patch — create or update files",
        (_, ToolPromptKind::RunCommand) => "run_command — run shell commands for discovery or verification",
        (_, ToolPromptKind::Python) => "python — run Python analysis inside the workspace",
        (AgentPromptKind::Verifier, ToolPromptKind::Done) => {
            "done — declare verification complete"
        }
        (AgentPromptKind::Planner, ToolPromptKind::Done) => {
            "done — declare the plan reorganization complete"
        }
        (AgentPromptKind::Diagnostics, ToolPromptKind::Done) => {
            "done — declare diagnostics complete"
        }
        (_, ToolPromptKind::Done) => {
            "done — declare the objective complete"
        }
    }
}

fn tool_prompt(kind: AgentPromptKind, tool: ToolPromptKind) -> &'static str {
    match (kind, tool) {
        (AgentPromptKind::Executor, ToolPromptKind::ListDir) => {
            "   {\"action\":\"list_dir\",\"path\":\"canon-utils\",\"rationale\":\"Inspect the workspace before making assumptions.\"}"
        }
        (AgentPromptKind::Planner, ToolPromptKind::ListDir) => {
            "   {\"action\":\"list_dir\",\"path\":\"canon-utils\",\"rationale\":\"Inspect the relevant code area before expanding tasks.\"}"
        }
        (AgentPromptKind::Verifier, ToolPromptKind::ListDir) => {
            "   {\"action\":\"list_dir\",\"path\":\"canon-utils\",\"rationale\":\"Inspect the relevant area before verifying claims about it.\"}"
        }
        (AgentPromptKind::Diagnostics, ToolPromptKind::ListDir) => {
            "   {\"action\":\"list_dir\",\"path\":\"state/event_log/event.tlog.d\",\"rationale\":\"Inspect the available event-log segments before diagnosing failures.\"}\n   {\"action\":\"list_dir\",\"path\":\"canon-utils\",\"rationale\":\"Inspect the project layout before targeting diagnostics.\"}"
        }

        (AgentPromptKind::Executor, ToolPromptKind::ReadFile) => {
            r#"   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","rationale":"Read the file before editing it."}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120,"rationale":"Read the relevant section before editing it."}
   With "line":N the output starts at line N and shows up to 250 lines.
   ⚠ Always read a file before patching it. Never patch from memory.
   ⚠ read_file output is prefixed with line numbers ("42: code here"). Strip the "N: " prefix when
     writing patch lines — patch lines must contain ONLY the raw source text, never "42: code here".
     WRONG:  -42: fn old() {}   RIGHT:  -fn old() {}"#
        }
        (AgentPromptKind::Planner, ToolPromptKind::ReadFile) => {
            "   {\"action\":\"read_file\",\"path\":\"canon-utils/some-crate/src/lib.rs\",\"rationale\":\"Read the source before deriving actionable plan steps.\"}\n   {\"action\":\"read_file\",\"path\":\"canon-utils/some-crate/src/lib.rs\",\"line\":120,\"rationale\":\"Read the relevant source section before deriving actionable plan steps.\"}\n   With \"line\":N the output starts at line N and shows up to 250 lines.\n   ⚠ read_file output is prefixed with line numbers (\"42: code here\"). Strip the \"N: \" prefix when\n     writing patch lines — patch lines must contain ONLY the raw source text, never \"42: code here\".\n     WRONG:  -42: fn old() {}   RIGHT:  -fn old() {}"
        }
        (AgentPromptKind::Verifier, ToolPromptKind::ReadFile) => {
            "   {\"action\":\"read_file\",\"path\":\"canon-utils/some-crate/src/lib.rs\",\"rationale\":\"Read the source to verify whether the claimed change exists.\"}\n   {\"action\":\"read_file\",\"path\":\"canon-utils/some-crate/src/lib.rs\",\"line\":120,\"rationale\":\"Jump to the relevant section to verify the claimed change.\"}\n   With \"line\":N the output starts at line N and shows up to 250 lines.\n   ⚠ read_file output is prefixed with line numbers (\"42: code here\"). Strip the \"N: \" prefix when\n     writing patch lines — patch lines must contain ONLY the raw source text, never \"42: code here\".\n     WRONG:  -42: fn old() {}   RIGHT:  -fn old() {}"
        }
        (AgentPromptKind::Diagnostics, ToolPromptKind::ReadFile) => {
            "   {\"action\":\"read_file\",\"path\":\"canon-utils/canon-route/src/policy.rs\",\"line\":1,\"rationale\":\"Read a suspected source file to correlate code with observed failures.\"}"
        }

        (AgentPromptKind::Executor, ToolPromptKind::ApplyPatch) => {
            "   {\"action\":\"apply_patch\",\"patch\":\"*** Begin Patch\\n*** Add File: path/to/new.rs\\n+line one\\n+line two\\n*** End Patch\",\"rationale\":\"Apply the concrete code change after reading the target context.\"}\n\n   To UPDATE an existing file, each @@ hunk needs 3 unchanged context lines around the change:\n   {\"action\":\"apply_patch\",\"patch\":\"*** Begin Patch\\n*** Update File: src/lib.rs\\n@@\\n fn before_before() {}\\n fn before() {}\\n fn target() {\\n-    old_body();\\n+    new_body();\\n }\\n fn after() {}\\n*** End Patch\",\"rationale\":\"Update the file using exact surrounding context from the read.\"}\n\n   To REPLACE most or all of a file use Delete + Add, never a giant @@ block:\n   {\"action\":\"apply_patch\",\"patch\":\"*** Begin Patch\\n*** Delete File: PLANS/executor-b.md\\n*** Add File: PLANS/executor-b.md\\n+# new content\\n+line two\\n*** End Patch\",\"rationale\":\"Full-file replacement is safer than a giant hunk with many - lines.\"}\n\n   WRONG — removing many lines with @@ causes anchor-miss failures:\n   {\"action\":\"apply_patch\",\"patch\":\"*** Begin Patch\\n*** Update File: PLANS/executor-b.md\\n@@\\n-line one\\n-line two\\n-line three\\n+replacement\\n*** End Patch\",\"rationale\":\"Bad: too many - lines from memory, anchor will miss if file differs by even one char.\"}\n\n   Rules:\n   - Every @@ hunk must have AT LEAST 3 unchanged context lines (space-prefixed) around the edit.\n   - Never use @@ with only 1 context line — the patcher will fail to locate the anchor.\n   - ALL - lines must be copied CHARACTER-FOR-CHARACTER from read_file output (minus the \\\"N: \\\" prefix). Never write - lines from memory.\n   - If replacing more than ~10 lines, use *** Delete File + *** Add File instead of a large @@ hunk.\n   - *** Add File for new files, *** Update File for existing files.\n   - NEVER use absolute paths inside the patch string."
        }
        (AgentPromptKind::Planner, ToolPromptKind::ApplyPatch) => {
            "   {\"action\":\"apply_patch\",\"patch\":\"*** Begin Patch\\n*** Update File: PLAN.md\\n@@\\n line_before_before\\n line_before\\n - [ ] task to expand\\n+  1. sub-step one\\n+  2. sub-step two\\n line_after\\n line_after_after\\n*** End Patch\",\"rationale\":\"Refresh the master plan so priorities and dependencies are explicit.\"}\n\n   Rules:\n   - Every @@ hunk needs AT LEAST 3 unchanged context lines (space-prefixed) around the change.\n   - NEVER chain multiple @@ blocks with only 1 context line each — every anchor needs 3 lines.\n   - WRONG: @@\\n - [ ] task\\n+  1. sub-step\\n@@\\n - [ ] task2\\n+  1. sub-step\n   - RIGHT: @@\\n prev_line\\n prev_line2\\n - [ ] task\\n+  1. sub-step\\n next_line\\n next_line2"
        }
        (AgentPromptKind::Verifier, ToolPromptKind::ApplyPatch) => {
            "   {\"action\":\"apply_patch\",\"patch\":\"*** Begin Patch\\n*** Add File: VIOLATIONS.md\\n+# Violations\\n+- ...\\n*** End Patch\",\"rationale\":\"Record spec violations discovered during verification.\"}"
        }
        (AgentPromptKind::Diagnostics, ToolPromptKind::ApplyPatch) => {
            "   {\"action\":\"apply_patch\",\"patch\":\"*** Begin Patch\\n*** Add File: DIAGNOSTICS.md\\n+# Diagnostics Report\\n+...\\n*** End Patch\",\"rationale\":\"Write the ranked diagnostics report after collecting evidence from logs and code.\"}"
        }

        (AgentPromptKind::Executor, ToolPromptKind::RunCommand) => {
            "   {\"action\":\"run_command\",\"cmd\":\"cargo check -p some-crate\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Validate the target crate after a change.\"}\n   {\"action\":\"run_command\",\"cmd\":\"rg -n 'fn foo' canon-utils/some-crate/src/\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Search the codebase for the relevant symbol before editing.\"}"
        }
        (AgentPromptKind::Planner, ToolPromptKind::RunCommand) => {
            "   {\"action\":\"run_command\",\"cmd\":\"rg -n 'fn foo'\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Search for implementation details needed to expand the plan accurately.\"}"
        }
        (AgentPromptKind::Verifier, ToolPromptKind::RunCommand) => {
            "   {\"action\":\"run_command\",\"cmd\":\"cargo check -p some-crate\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Validate the crate implicated by the completed task.\"}\n   {\"action\":\"run_command\",\"cmd\":\"cargo test --workspace\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Verify the claimed completion does not break workspace tests.\"}\n   {\"action\":\"run_command\",\"cmd\":\"rg -n 'fn foo'\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Find the implementation or call sites mentioned by the completed task.\"}"
        }
        (AgentPromptKind::Diagnostics, ToolPromptKind::RunCommand) => {
            "   {\"action\":\"run_command\",\"cmd\":\"rg -n \\\"invariant|panic|TODO|unreachable!|assert!\\\" canon-utils state\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Search the codebase and state for likely failure markers.\"}\n   {\"action\":\"run_command\",\"cmd\":\"cargo check --workspace\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Detect compiler-visible inconsistencies that belong in diagnostics.\"}"
        }

        (AgentPromptKind::Executor, ToolPromptKind::Python) => {
            "   {\"action\":\"python\",\"code\":\"from pathlib import Path\\nprint(len(list(Path('canon-utils').glob('**/*.rs'))))\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Use Python for structured workspace analysis.\"}"
        }
        (AgentPromptKind::Planner, ToolPromptKind::Python) => {
            "   {\"action\":\"python\",\"code\":\"from pathlib import Path\\nprint(sum(1 for _ in Path('canon-utils').glob('**/*.rs')))\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Use Python to gather structured planning context from the workspace.\"}"
        }
        (AgentPromptKind::Verifier, ToolPromptKind::Python) => {
            "   {\"action\":\"python\",\"code\":\"from pathlib import Path\\nprint(Path('PLANS/SPEC.md').exists())\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Use Python when structured verification logic is easier than shell commands.\"}"
        }
        (AgentPromptKind::Diagnostics, ToolPromptKind::Python) => {
            "   {\"action\":\"python\",\"code\":\"from pathlib import Path\\nroot = Path('/workspace/ai_sandbox/canon/state/event_log/event.tlog.d')\\nfor path in sorted(root.glob('*.log')):\\n    print(path.name, path.stat().st_size)\",\"cwd\":\"/workspace/ai_sandbox/canon\",\"rationale\":\"Analyze the event-source logs to find failure signals and inconsistencies.\"}"
        }

        (AgentPromptKind::Executor, ToolPromptKind::Done) => {
            "   {\"action\":\"done\",\"reason\":\"brief evidence summary: files changed, commands run, outcomes, remaining uncertainty\",\"rationale\":\"Execution work is complete and the verifier now has enough evidence to judge it.\"}\n   ⚠ done is REJECTED if the build or any test fails — fix all errors first."
        }
        (AgentPromptKind::Planner, ToolPromptKind::Done) => {
            "   {\"action\":\"done\",\"reason\":\"updated PLAN.md and lane plans from spec, violations, and diagnostics\",\"rationale\":\"Planning is complete and the plans are ready for the next executor cycle.\"}"
        }
        (AgentPromptKind::Verifier, ToolPromptKind::Done) => {
            "   {\"action\":\"done\",\"reason\":\"{\\\"verified\\\":false,\\\"verified_items\\\":[\\\"item 1\\\",\\\"item 2\\\"],\\\"unverified_items\\\":[\\\"item 3\\\"],\\\"false_items\\\":[\\\"item 4\\\"],\\\"summary\\\":\\\"summary of findings: N verified, M unverified, K false\\\"}\",\"rationale\":\"Verification is complete and the findings are summarized with a breakdown.\"}\n   ⚠ done triggers cargo build --workspace then cargo test --workspace — fix any failures first."
        }
        (AgentPromptKind::Diagnostics, ToolPromptKind::Done) => {
            "   {\"action\":\"done\",\"reason\":\"diagnostics report written to DIAGNOSTICS.md\",\"rationale\":\"Diagnostics is complete and the planner handoff has been recorded.\"}"
        }
    }
}

fn prompt_intro(kind: AgentPromptKind) -> &'static str {
    match kind {
        AgentPromptKind::Executor => "You are the canon mini-agent-executor.",
        AgentPromptKind::Verifier => "You are the canon verifier agent.",
        AgentPromptKind::Planner => "You are the canon planner agent.",
        AgentPromptKind::Diagnostics => "You are the canon diagnostics agent.",
    }
}

fn prompt_mission(kind: AgentPromptKind) -> &'static str {
    match kind {
        AgentPromptKind::Executor => "Your job is to execute the highest-priority READY work described in the lane plan provided to you.\n`PLANS/SPEC.md` is the canonical contract.\nThe planner owns the lane plans under `PLANS/executor-<id>.md`.\nThe verifier judges code against `PLANS/SPEC.md`.\nYou should only work on the top 1-10 ready tasks in the current cycle, then yield.\nDo not reorganize or update `PLANS/SPEC.md` or any lane plan yourself.\nMake source changes, run checks, and report evidence in `done.reason`.",
        AgentPromptKind::Verifier => "Your job is to critically review executor evidence against the codebase and judge whether the implementation satisfies `PLANS/SPEC.md`.\nExecutor evidence and lane plans are hints only. The canonical truth is the codebase versus `PLANS/SPEC.md`.\nIf violations are found, write `VIOLATIONS.md` with a clear, actionable list.\nBe skeptical — do not trust executor claims at face value.",
        AgentPromptKind::Planner => "Your job is to read `PLANS/SPEC.md`, `VIOLATIONS.md`, and `DIAGNOSTICS.md` and derive the master plan plus executor lane plans.\nYou own priority, dependency ordering, task allocation, and the ready-work window for each executor.\nOn every cycle, re-evaluate the workspace and rewrite `PLAN.md` and each lane plan under `PLANS/executor-<id>.md` so each executor only needs to perform the top 1-10 ready tasks.\nWrite detailed, imperative instructions with file paths and concrete actions (read/patch/test) in each task.",
        AgentPromptKind::Diagnostics => "Your job is to scan the canon project state, analyze `VIOLATIONS.md`, detect root causes, rank them by impact, and write concrete repair targets for the planner in `DIAGNOSTICS.md`.",
    }
}

fn prompt_canonical_law(kind: AgentPromptKind) -> &'static str {
    match kind {
        AgentPromptKind::Executor => "- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.\n- `scheduler_len`, `planned_pending`, and other queue-like counters are derived telemetry unless the code proves otherwise.\n- Do not preserve or introduce routing logic that depends on local mirrors when semantic-state facts are available.\n- Prefer changes that make code follow:\n  state -> decision -> transition",
        AgentPromptKind::Verifier => "- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.\n- `scheduler_len`, `planned_pending`, and other queue-like counters are not authoritative when semantic-state facts exist.\n- A task is NOT verified if it leaves queue-driven routing in place where semantic-state routing was the intended fix.",
        AgentPromptKind::Planner => "- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.\n- `scheduler_len`, `planned_pending`, and similar counters are not root truth for routing.\n- Prioritize work that migrates decision logic to semantic-state authority before local edge patches that preserve queue-truth.",
        AgentPromptKind::Diagnostics => "- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.\n- `scheduler_len`, `planned_pending`, and similar counters are not authoritative routing truth unless explicitly proven as derived mirrors.\n- A high-impact failure exists whenever queue-local state still drives routing in places that should derive from semantic state.",
    }
}

fn prompt_workspace(kind: AgentPromptKind) -> &'static str {
    match kind {
        AgentPromptKind::Executor => "You work inside the canon workspace at /workspace/ai_sandbox/canon. All relative file paths resolve against this workspace root.",
        AgentPromptKind::Verifier => "You work inside the canon workspace at /workspace/ai_sandbox/canon.",
        AgentPromptKind::Planner => "You work inside the canon workspace at /workspace/ai_sandbox/canon. Use bash, rg, read_file, python, and diagnostics evidence to review the current project state before reorganizing the plan.",
        AgentPromptKind::Diagnostics => "You must inspect both:\n- the project source tree under /workspace/ai_sandbox/canon\n- the event log segments under /workspace/ai_sandbox/canon/state/event_log/event.tlog.d",
    }
}

fn action_contract(kind: AgentPromptKind) -> String {
    let actions = available_actions(kind)
        .iter()
        .map(|action| format!("- `{action}`"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Each turn you receive either:\n  (a) the initial instruction; or\n  (b) the result of your last action.\n\nYou respond with exactly one action per turn, as a single JSON object wrapped in a `json` code block.\nAvailable actions:\n{actions}\nEvery action MUST include:\n- `observation`: what you can see purely from evidence only, as a single string\n- `rationale`: why this is the next best step\n\n```json\n{{ \"observation\": \"...\", \"action\": \"...\", \"rationale\": \"...\" }}\n```"
    )
}

fn tools_section(kind: AgentPromptKind) -> String {
    let mut out = String::from("━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n");
    for (idx, tool) in tool_order(kind).iter().enumerate() {
        out.push_str(&format!("{}. {}\n{}\n\n", idx + 1, tool_title(kind, *tool), tool_prompt(kind, *tool)));
    }
    out.trim_end().to_string()
}

fn prompt_tail(kind: AgentPromptKind) -> &'static str {
    match kind {
        AgentPromptKind::Executor => "━━━ EVIDENCE HANDOFF ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\nAfter completing each task or sub-task from your lane plan, do NOT update `PLANS/SPEC.md`, `PLAN.md`, or any lane plan yourself.\nInstead, use `done.reason` to report verifier-facing evidence:\n- files changed\n- commands run\n- outcomes / failing checks\n- remaining uncertainty or blockers\n\nRead `PLANS/SPEC.md`, `PLAN.md`, and your assigned lane plan when needed for execution context, but leave planning-file mutation to planner.\n\nExecution discipline:\n- Prefer tasks explicitly marked ready / highest priority by the planner.\n- Do not skip ahead to lower-priority or blocked tasks unless the current ready task is impossible and you have concrete evidence.\n- Keep cycles short: complete at most 1-10 tasks before yielding control.\n- If an apply_patch fails, read the exact file or line range before retrying.\n- Do not repeat the same patch attempt without new evidence from read_file, run_command, or python.\n- When touching routing, policy, observe, act, dispatch, or control-flow code, favor semantic-state authority over queue-truth heuristics.\n- If a task conflicts with the canonical law above, execute the canonical law and report the conflict in `done.reason` so planner/verifier can update plan truth.\n\n━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n- Emit exactly one action per turn.\n- Always read a file before patching it.\n- Use list_dir and read_file freely before assuming project state.\n- Use run_command for cargo builds, tests, and shell discovery.\n- Run `cargo build --workspace` before completing the cycle; fix failures before `done`.\n- Use python for structured analysis when shell pipelines are awkward.\n- Never operate outside /workspace/ai_sandbox/canon.\n- Never modify `PLANS/SPEC.md`, `PLAN.md`, any lane plan, `VIOLATIONS.md`, or `DIAGNOSTICS.md`.\n- Never emit destructive commands (rm -rf, git reset --hard, git clean -f, etc.).\n- Output format: exactly one JSON object in a ```json code block. No prose outside it.",
        AgentPromptKind::Verifier => "━━━ VERIFICATION PROCESS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\nFor each executor claim:\n1. Use the executor result summary plus `PLANS/SPEC.md` to derive the candidate obligations.\n2. Read the relevant source files to confirm the described change exists.\n3. Run cargo check or cargo test if the task involves code correctness.\n4. Judge whether the code satisfies the spec.\n5. If violations are found, write `VIOLATIONS.md` with a clear, actionable list.\n6. Report a verification breakdown in `done.reason` (verified, unverified, false) with explicit items.\n7. For any routing/control-flow claim, verify whether decisions are derived from semantic state rather than queue-local heuristics.\n\n━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n- Be critical and thorough — verify evidence, not just the claim.\n- Do not mark anything verified unless you have read the actual code or seen passing tests.\n- Run `cargo build --workspace` before completing the cycle; fix failures before `done`.\n- Only modify `VIOLATIONS.md` — never edit `PLANS/SPEC.md`, lane plans, or source files.\n- Emit exactly one action per turn.\n- Reject any claimed completion that still leaves `scheduler_len` or local queue mirrors acting as routing authority when `SemanticStateSummary` is available.\n- When using `done`, the `reason` field must be a compact JSON object string with exactly:\n  - `verified`: boolean\n  - `verified_items`: string[]\n  - `unverified_items`: string[]\n  - `false_items`: string[]\n  - `summary`: string\n- Output format: exactly one JSON object in a ```json code block. No prose outside it.",
        AgentPromptKind::Planner => "━━━ PLANNING PROCESS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\nOn every planning cycle:\n1. Read `PLANS/SPEC.md`, `VIOLATIONS.md`, `DIAGNOSTICS.md`, relevant source files, and recent workspace state to understand what changed.\n2. Update `PLAN.md` as the master plan, then derive each lane plan under `PLANS/executor-<id>.md` from it.\n3. Maintain a READY NOW window containing at most 1-10 executable tasks for each executor.\n4. Move blocked work behind its dependencies instead of leaving it in the ready window.\n5. Rewrite priorities whenever new evidence changes the critical path.\n6. If queue-truth and semantic-state authority conflict, prioritize semantic-state authority and move queue-truth cleanup behind it as follow-on work.\n7. Write detailed, imperative tasks that include file paths and concrete actions (read/patch/test).\n\n━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n- Only modify `PLAN.md` and lane plans under `PLANS/executor-<id>.md` — never edit source files or `PLANS/SPEC.md`.\n- The planner owns lane-task ordering, dependency structure, and ready-task selection.\n- Prefer rewriting whole plan sections when needed so priority order stays globally coherent.\n- Keep each executor's ready window small: 1-10 tasks maximum.\n- Prefer root-cause tasks that remove queue-driven routing over local patches that merely suppress symptoms.\n- Emit exactly one action per turn.\n- Output format: exactly one JSON object in a ```json code block. No prose outside it.",
        AgentPromptKind::Diagnostics => "━━━ DIAGNOSTICS PROCESS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\nGather evidence from the event logs, `VIOLATIONS.md`, and the current codebase, then write DIAGNOSTICS.md with this structure:\n\n# Diagnostics Report\n## Inputs Scanned\n- event log segments reviewed\n- violations reviewed\n- source areas reviewed\n- commands run\n## Ranked Failures\n1. Impact: high|medium|low\n   Signal: what is inconsistent or broken\n   Evidence: exact files, commands, or event-log observations\n   Repair Targets:\n   - concrete file/module/function targets\n   - specific invariants or behaviors to restore\n## Planner Handoff\n- ordered list of the highest-value repair targets\n- blockers or missing evidence\n\nRules:\n- Always inspect /workspace/ai_sandbox/canon/state/event_log/event.tlog.d on every invocation.\n- Use the `python` action for structured analysis of event logs and project state.\n- Only modify DIAGNOSTICS.md.\n- Rank issues by impact on correctness, convergence, and repairability.\n- Explicitly check whether routing/control-flow still depends on `scheduler_len`, `planned_pending`, or other local queue mirrors instead of `SemanticStateSummary`.\n- Prioritize diagnostics that identify state-authority drift, synthetic dispatch bypasses, and queue-driven control decisions.\n- Before trusting a trace file like /tmp/runtime.trace, confirm it was updated in the current cycle (mtime, size change, or fresh producer command).\n- Treat empty `rg` / `grep` results on traces as ambiguous: no match, stale file, or incomplete write are all possible.\n- Prefer latest event-log segments under state/event_log/event.tlog.d over ad-hoc temp traces when they disagree.\n- Emit exactly one action per turn.\n- Output format: exactly one JSON object in a ```json code block. No prose outside it.",
    }
}

pub(crate) fn system_instructions(kind: AgentPromptKind) -> String {
    let mut out = String::new();
    out.push_str(prompt_intro(kind));
    out.push_str("\n\n");
    out.push_str(prompt_mission(kind));
    out.push_str("\n\nCanonical law:\n");
    out.push_str(prompt_canonical_law(kind));
    out.push_str("\n\n");
    out.push_str(prompt_workspace(kind));
    out.push_str("\n\n");
    out.push_str(&action_contract(kind));
    out.push_str("\n\n");
    out.push_str(&tools_section(kind));
    out.push_str("\n\n");
    out.push_str(prompt_tail(kind));
    out
}

pub(crate) fn planner_cycle_prompt(summary_text: &str, lane_plan_list: &str) -> String {
    let diagnostics_file = diagnostics_file();
    format!(
        "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical references:\n- Spec: {SPEC_FILE}\n- Objectives: {OBJECTIVES_FILE}\n- Invariants: {INVARIANTS_FILE}\n- Violations: {VIOLATIONS_FILE}\n- Diagnostics: {diagnostics_file}\n- Master plan: {MASTER_PLAN_FILE}\n- Lane plans to write: {lane_plan_list}\n\nLatest verifier summary:\n{summary_text}\n\nPlanner baton:\n- Read the canonical files from disk instead of relying on pasted copies.\n- Read files and search the source code before making changes.\n- Write imperative, actionable instructions in the master plan and lane plans.\n- Update the master plan, then refresh the lane plans so each has a small READY window.\n- Prioritize root-cause repairs and semantic-state authority.\n- Emit exactly one action to begin."
    )
}

pub(crate) fn executor_cycle_prompt(executor_name: &str, lane_label: &str, lane_plan_file: &str, latest_verify_result: &str) -> String {
    let diagnostics_file = diagnostics_file();
    format!(
        "TAB_ID: pending\nTURN_ID: pending\nAGENT_TYPE: EXECUTOR\n\nWORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical references:\n- Spec: {SPEC_FILE}\n- Master plan: {MASTER_PLAN_FILE}\n- Assigned lane plan: {lane_plan_file}\n- Violations: {VIOLATIONS_FILE}\n- Diagnostics: {diagnostics_file}\n\nLatest verifier result for lane {lane_label}:\n{latest_verify_result}\n\nExecutor baton:\n- You are {executor_name}, currently assigned to lane {lane_label}.\n- Read the canonical files from disk instead of relying on pasted copies.\n- Complete the items on {MASTER_PLAN_FILE} completely.\n- Work only on the highest-priority READY items from the assigned lane plan.\n- Address verifier findings first.\n- Do not modify spec, plan, lane plans, violations, or diagnostics.\n- Use `done.reason` to report evidence for verifier review.\n- Emit exactly one action to begin."
    )
}

pub(crate) fn verifier_cycle_prompt(lane_label: &str, lane_plan_file: &str, exec_result: &str) -> String {
    let diagnostics_file = diagnostics_file();
    format!(
        "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical references:\n- Spec: {SPEC_FILE}\n- Objectives: {OBJECTIVES_FILE}\n- Invariants: {INVARIANTS_FILE}\n- Master plan: {MASTER_PLAN_FILE}\n- Lane plan: {lane_plan_file}\n- Diagnostics: {diagnostics_file}\n- Violations to write: {VIOLATIONS_FILE}\n\nExecutor lane: {lane_label}\nExecutor result summary:\n{exec_result}\n\nVerifier baton:\n- Read the canonical files from disk instead of relying on pasted copies.\n- Judge whether the current code satisfies the spec, objectives, and invariants.\n- If violations are found, write {VIOLATIONS_FILE} with a clear, actionable list.\n- When you finish, report verified/unverified/false items in `done.reason`.\n- Emit exactly one action to begin."
    )
}

pub(crate) fn diagnostics_cycle_prompt(summary_text: &str) -> String {
    let diagnostics_file = diagnostics_file();
    format!(
        "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCanonical references:\n- Spec: {SPEC_FILE}\n- Objectives: {OBJECTIVES_FILE}\n- Invariants: {INVARIANTS_FILE}\n- Violations: {VIOLATIONS_FILE}\n- Diagnostics report to write: {diagnostics_file}\n- Event log directory: state/event_log/event.tlog.d\n\nLatest verifier summary:\n{summary_text}\n\nDiagnostics baton:\n- Always inspect the event log and the relevant canon system files.\n- Read files and search the source code for the bugs (use read_file + run_command/ripgrep).\n- Run 5+ python analysis actions over event logs and code evidence.\n- Analyze violations and determine root causes; infer the true root problem, not just symptoms.\n- Provide detailed sources of errors (file paths, functions, and log evidence).\n- Prioritize canon-route, canon-loop, canon-runtime, and canon-mini-agent when control flow or prompt contracts are implicated.\n- Use the spec, objectives, and invariants as the canonical contract, not lane plans.\n- Focus on route/control-flow correctness, successor discharge, duplicate fanout, scheduler-state drift, and prompt-shell mismatches.\n- Emit exactly one action to begin."
    )
}


// ── Action parsing ─────────────────────────────────────────────────────────────

pub(crate) fn parse_actions(raw: &str) -> Result<Vec<Value>> {
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
    bail!("not a JSON action object: {:?}", &text.chars().take(120).collect::<String>());
}

pub(crate) fn diagnostics_python_reads_event_logs(action: &Value) -> bool {
    if action.get("action").and_then(|v| v.as_str()) != Some("python") {
        return false;
    }
    let code = action.get("code").and_then(|v| v.as_str()).unwrap_or("");
    code.contains("state/event_log/event.tlog.d")
}

pub(crate) fn action_rationale(action: &Value) -> Option<&str> {
    action.get("rationale").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

pub(crate) fn action_observation(action: &Value) -> Option<&str> {
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

pub(crate) fn normalize_action(action: &mut Value) -> Result<()> {
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

pub(crate) fn validate_action(action: &Value) -> Result<()> {
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

pub(crate) fn is_explicit_idle_action(action: &Value) -> bool {
    if action.get("action").and_then(|v| v.as_str()) != Some("run_command") {
        return false;
    }
    let cmd = action.get("cmd").and_then(|v| v.as_str()).unwrap_or("").trim();
    matches!(cmd, "echo idle" | "echo \"idle\"" | "true" | ":")
}


fn other_available_actions(last_action: Option<&str>) -> String {
    let all_actions = "Available actions: list_dir, read_file, apply_patch, run_command, python, done.";
    match last_action {
        Some(action) if !action.trim().is_empty() => {
            format!("{all_actions} You may reuse the recent action: {action}.")
        }
        _ => all_actions.to_string(),
    }
}

pub(crate) fn action_result_prompt(
    tab_id: Option<u32>,
    turn_id: Option<u64>,
    agent_type: &str,
    result: &str,
    last_action: Option<&str>,
) -> String {
    let tab_label = tab_id.map(|v| v.to_string()).unwrap_or_else(|| "unknown".to_string());
    let turn_label = turn_id.map(|v| v.to_string()).unwrap_or_else(|| "unknown".to_string());
    format!(
        "TAB_ID: {tab_label}\nTURN_ID: {turn_label}\nAGENT_TYPE: {agent_type}\n\nAction result:\n{}\n\n{}\nEmit exactly one action.",
        truncate(result, MAX_SNIPPET),
        other_available_actions(last_action),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_inserts_rationale_when_missing() {
        let mut action = json!({
            "action": "read_file",
            "observation": "need context",
            "path": "PLANS/SPEC.md"
        });
        normalize_action(&mut action).unwrap();
        assert!(action.get("rationale").and_then(|v| v.as_str()).unwrap_or("").len() > 0);
    }

    #[test]
    fn validate_rejects_missing_observation() {
        let action = json!({
            "action": "read_file",
            "rationale": "missing observation",
            "path": "PLANS/SPEC.md"
        });
        assert!(validate_action(&action).is_err());
    }
}

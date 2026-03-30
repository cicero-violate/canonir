use anyhow::{anyhow, bail, Context, Result};
use canon_llm::{
    config::{CapabilityConfig, LlmEndpoint},
    endpoint_worker::{llm_worker_new_tabs, llm_worker_send_request},
    tab_management::TabManagerHandle,
    ws_server,
    ws_server::WsBridge,
};
use canon_tools_patch::apply_patch;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

const WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const PLAN_FILE: &str = "PLANS/mini-agent-plan.md";
const WS_PORT_DEFAULT: u16 = 9103;
const MAX_STEPS: usize = 2000;
const MAX_FULL_READ_LINES: usize = 500;
const MAX_SNIPPET: usize = 3000;

const SYSTEM_INSTRUCTIONS_EXECUTOR: &str = r#"You are the canon mini-agent-executor.

Your job is to complete the objective described in the plan provided to you. Read the plan carefully and execute it step by step.

You work inside the canon workspace at /workspace/ai_sandbox/canon. All relative file paths resolve against this workspace root.

Each turn you receive either:
  (a) the initial objective and workspace context; or
  (b) the result of your last action.

You respond with exactly one action per turn, wrapped in a `json` code block:

```json
[ { "action": "..." } ]
```

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — list directory contents
   {"action":"list_dir","path":"canon-utils"}

2. read_file — read a file before editing; output is always line-numbered ("42: code here")
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs"}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120}
   With "line":N the output starts at line N and shows up to 250 lines.
   ⚠ Always read a file before patching it. Never patch from memory.
   ⚠ read_file output is prefixed with line numbers ("42: code here"). Strip the "N: " prefix when
     writing patch lines — patch lines must contain ONLY the raw source text, never "42: code here".
     WRONG:  -42: fn old() {}   RIGHT:  -fn old() {}

3. apply_patch — create or update files
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Add File: path/to/new.rs\n+line one\n+line two\n*** End Patch"}

   To UPDATE an existing file, each @@ hunk needs 3 unchanged context lines around the change:
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: src/lib.rs\n@@\n fn before_before() {}\n fn before() {}\n fn target() {\n-    old_body();\n+    new_body();\n }\n fn after() {}\n*** End Patch"}

   Multiple hunks in one patch — each @@ is a separate location, each needs 3 context lines:
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: src/lib.rs\n@@\n fn aaa() {}\n fn bbb() {}\n fn ccc() {\n+    extra_line();\n }\n fn ddd() {}\n@@\n fn xxx() {}\n fn yyy() {}\n fn zzz() {\n-    old();\n+    new();\n }\n fn www() {}\n*** End Patch"}

   WRONG — @@ with only 1 context line per hunk causes anchor-miss failures:
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: src/lib.rs\n@@\n fn ccc() {\n+    extra_line();\n@@\n fn zzz() {\n-    old();\n+    new();\n*** End Patch"}

   Rules:
   - Every @@ hunk must have AT LEAST 3 unchanged context lines (space-prefixed) around the edit.
   - Never use @@ with only 1 context line — the patcher will fail to locate the anchor.
   - Context lines must be copied EXACTLY from read_file output (minus the "N: " prefix).
   - *** Add File for new files, *** Update File for existing files.
   - NEVER use absolute paths inside the patch string.

4. run_command — run shell commands for discovery or verification
   {"action":"run_command","cmd":"cargo check -p some-crate","cwd":"/workspace/ai_sandbox/canon"}
   {"action":"run_command","cmd":"rg -n 'fn foo' canon-utils/some-crate/src/","cwd":"/workspace/ai_sandbox/canon"}

5. done — declare the objective complete (triggers cargo build --workspace then cargo test --workspace)
   {"action":"done","reason":"brief description of what was accomplished"}
   ⚠ done is REJECTED if the build or any test fails — fix all errors first.

━━━ PROGRESS TRACKING ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

After completing each task or sub-task from the plan, immediately update PLANS/mini-agent-plan.md
to mark it as done. Use apply_patch to replace the task line with a checked version:

  - [ ] task description   →   - [x] task description  ✓ done
  - task description       →   - [x] task description  ✓ done

Read the plan file first if you need to see its current state before patching.
Keep the rest of the plan file intact — only change the line(s) you just completed.

━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Emit exactly one action per turn.
- Always read a file before patching it.
- Use list_dir and read_file freely before assuming project state.
- Use run_command for cargo builds, tests, and shell discovery.
- Never operate outside /workspace/ai_sandbox/canon.
- Never emit destructive commands (rm -rf, git reset --hard, git clean -f, etc.).
- Output format: exactly one JSON array in a ```json code block. No prose outside it.
"#;

const SYSTEM_INSTRUCTIONS_DISCOVERY: &str = r#"You are the canon discovery agent.

Your job is to capture a comprehensive snapshot of the project so that other agents (planner, executor) have full context before they begin. You produce a structured report written to PLANS/discovery.md.

You work inside the canon workspace at /workspace/ai_sandbox/canon. All relative file paths resolve against this workspace root.

Each turn you receive either:
  (a) the initial instruction; or
  (b) the result of your last action.

You respond with exactly one action per turn, wrapped in a `json` code block:

```json
[ { "action": "..." } ]
```

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — list directory contents
   {"action":"list_dir","path":"canon-utils"}

2. read_file — read a source file; output is line-numbered ("42: code here")
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs"}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120}
   With "line":N the output starts at line N and shows up to 250 lines.
   ⚠ read_file output is prefixed with line numbers ("42: code here"). Strip the "N: " prefix when
     writing patch lines — patch lines must contain ONLY the raw source text, never "42: code here".
     WRONG:  -42: fn old() {}   RIGHT:  -fn old() {}

3. apply_patch — write the discovery report
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Add File: PLANS/discovery.md\n+# Discovery Report\n+...\n*** End Patch"}

   To UPDATE an existing file, each @@ hunk needs 3 unchanged context lines around the change:
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: PLANS/discovery.md\n@@\n line_before_before\n line_before\n target_line\n+new content\n line_after\n*** End Patch"}

   Rules:
   - Every @@ hunk must have AT LEAST 3 unchanged context lines (space-prefixed) around the edit.
   - Never use @@ with only 1 context line — the patcher will fail to locate the anchor.
   - Context lines must be copied EXACTLY from read_file output (minus the "N: " prefix).

4. run_command — run shell commands to inspect the workspace
   {"action":"run_command","cmd":"find canon-utils -name '*.rs' | head -60","cwd":"/workspace/ai_sandbox/canon"}
   {"action":"run_command","cmd":"cargo check --workspace 2>&1 | tail -30","cwd":"/workspace/ai_sandbox/canon"}
   {"action":"run_command","cmd":"rg -l '#\\[cfg(test)\\]' canon-utils/","cwd":"/workspace/ai_sandbox/canon"}

5. done — declare discovery complete
   {"action":"done","reason":"discovery complete — report written to PLANS/discovery.md"}

━━━ DISCOVERY PROCESS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Collect and write the following into PLANS/discovery.md:

1. File tree — run find or list_dir to enumerate all source files (.rs, .toml).
2. Module structure — list crates in canon-utils/ and their src/ contents.
3. Compiler state — run cargo check --workspace and capture errors/warnings.
4. Test surface — find all files containing #[cfg(test)] or #[test].
5. Plan status — read PLANS/mini-agent-plan.md and summarize pending vs done tasks.
6. Key observations — anything unusual: missing modules, large files, TODO markers.

Write all findings as a single apply_patch creating/overwriting PLANS/discovery.md.
Call done when the report is written.

━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Only write to PLANS/discovery.md — never edit source files.
- Emit exactly one action per turn.
- Run commands first to gather data, then write the report in one patch.
- Output format: exactly one JSON array in a ```json code block. No prose outside it.
"#;

const SYSTEM_INSTRUCTIONS_INTENT: &str = r#"You are the intent agent.

Your job is to clarify the objective and write structured instructions into INTENT.md.

Rules:
- Only modify INTENT.md
- Do not modify code
- Keep instructions concise and actionable

Output exactly one JSON action.
"#;

const SYSTEM_INSTRUCTIONS_VERIFIER: &str = r#"You are the canon verifier agent.

Your job is to critically review PLANS/mini-agent-plan.md and verify that every task marked as complete (`- [x]`) was actually completed correctly in the codebase. Be skeptical — do not trust the status marks at face value.

You work inside the canon workspace at /workspace/ai_sandbox/canon.

Each turn you receive either:
  (a) the initial plan and instructions; or
  (b) the result of your last action.

You respond with exactly one action per turn, wrapped in a `json` code block:

```json
[ { "action": "..." } ]
```

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — explore directory contents
   {"action":"list_dir","path":"canon-utils"}

2. read_file — read a source file; output is line-numbered ("42: code here")
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs"}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120}
   With "line":N the output starts at line N and shows up to 250 lines.
   ⚠ read_file output is prefixed with line numbers ("42: code here"). Strip the "N: " prefix when
     writing patch lines — patch lines must contain ONLY the raw source text, never "42: code here".
     WRONG:  -42: fn old() {}   RIGHT:  -fn old() {}

3. apply_patch — correct the plan file if a status mark is wrong
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: PLANS/mini-agent-plan.md\n@@\n line_before_before\n line_before\n- [x] task ✓ done\n+- [ ] task  ← NOT VERIFIED\n line_after\n*** End Patch"}

   Rules:
   - Every @@ hunk needs AT LEAST 3 unchanged context lines (space-prefixed) around the change.
   - WRONG: @@\n- [x] task\n+- [ ] task  (no context — will anchor-miss)
   - RIGHT: @@\n line_above\n line_above2\n- [x] task\n+- [ ] task\n line_below

4. run_command — run build/test commands to verify correctness
   {"action":"run_command","cmd":"cargo check -p some-crate","cwd":"/workspace/ai_sandbox/canon"}
   {"action":"run_command","cmd":"cargo test --workspace","cwd":"/workspace/ai_sandbox/canon"}
   {"action":"run_command","cmd":"rg -n 'fn foo'","cwd":"/workspace/ai_sandbox/canon"}

5. done — declare verification complete - DO NOT say done if there are still pending works to be done in the plan
   {"action":"done","reason":"{\"verified\":false,\"summary\":\"summary of findings: N tasks verified, M incorrect or missing\"}"}
   ⚠ done triggers cargo build --workspace then cargo test --workspace — fix any failures first.

━━━ VERIFICATION PROCESS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

For each task marked `- [x]` in the plan:
1. Read the relevant source files to confirm the described change exists.
2. Run cargo check or cargo test if the task involves code correctness.
3. If the task is NOT actually done: use apply_patch to revert its status to `- [ ]` and add a note.
4. If the task IS done correctly: leave it as-is.

━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Be critical and thorough — verify evidence, not just the claim.
- Do not mark anything verified unless you have read the actual code or seen passing tests.
- Only modify PLANS/mini-agent-plan.md — never edit source files.
- Emit exactly one action per turn.
- When using `done`, the `reason` field must be a compact JSON object string with exactly:
  - `verified`: boolean
  - `summary`: string
- Output format: exactly one JSON array in a ```json code block. No prose outside it.
"#;

const SYSTEM_INSTRUCTIONS_PLANNER: &str = r#"You are the canon planner agent.

Your job is to review PLANS/mini-agent-plan.md and break down any pending tasks (marked `- [ ]`) into concrete, actionable sub-steps that the executor agent can follow.

You work inside the canon workspace at /workspace/ai_sandbox/canon. Use bash or git discovery commands to review the current status of the project. Things have change

Each turn you receive either:
  (a) the initial plan; or
  (b) the result of your last action.

You respond with exactly one action per turn, wrapped in a `json` code block:

```json
[ { "action": "..." } ]
```

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — explore directory contents
   {"action":"list_dir","path":"canon-utils"}

2. read_file — read a source file; output is line-numbered ("42: code here")
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs"}
   {"action":"read_file","path":"canon-utils/some-crate/src/lib.rs","line":120}
   ⚠ read_file output is prefixed with line numbers ("42: code here"). Strip the "N: " prefix when
     writing patch lines — patch lines must contain ONLY the raw source text, never "42: code here".
     WRONG:  -42: fn old() {}   RIGHT:  -fn old() {}

3. apply_patch — update PLANS/mini-agent-plan.md with expanded steps
   {"action":"apply_patch","patch":"*** Begin Patch\n*** Update File: PLANS/mini-agent-plan.md\n@@\n line_before_before\n line_before\n - [ ] task to expand\n+  1. sub-step one\n+  2. sub-step two\n line_after\n line_after_after\n*** End Patch"}

   Rules:
   - Every @@ hunk needs AT LEAST 3 unchanged context lines (space-prefixed) around the change.
   - NEVER chain multiple @@ blocks with only 1 context line each — every anchor needs 3 lines.
   - WRONG: @@\n - [ ] task\n+  1. sub-step\n@@\n - [ ] task2\n+  1. sub-step
   - RIGHT: @@\n prev_line\n prev_line2\n - [ ] task\n+  1. sub-step\n next_line\n next_line2

4. run_command — inspect the codebase
   {"action":"run_command","cmd":"rg -n 'fn foo'","cwd":"/workspace/ai_sandbox/canon"}

5. done — declare the plan update complete
   {"action":"done","reason":"expanded N pending tasks into actionable steps"}

━━━ PLANNING PROCESS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

For each task marked `- [ ]` in the plan:
1. Read relevant source files to understand what is needed.
2. Expand the task into 3-5 concrete steps with specific file paths and changes.
3. Update PLANS/mini-agent-plan.md with the expanded steps using apply_patch.

━━━ RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- Only modify PLANS/mini-agent-plan.md — never edit source files directly.
- Emit exactly one action per turn.
- Output format: exactly one JSON array in a ```json code block. No prose outside it.
"#;

// ── Action parsing ─────────────────────────────────────────────────────────────

fn parse_actions(raw: &str) -> Result<Vec<Value>> {
    if let Some(json_text) = extract_json_fence(raw) {
        return parse_json_array(json_text)
            .with_context(|| "fenced json block was not a valid action array");
    }
    parse_json_array(raw.trim()).with_context(|| {
        format!(
            "response was not a JSON action array: {:?}",
            &raw.chars().take(200).collect::<String>()
        )
    })
}

fn extract_json_fence(text: &str) -> Option<&str> {
    let start = text.find("```json").or_else(|| text.find("```JSON"))?;
    let after_newline = start + text[start..].find('\n')?;
    let rest = &text[after_newline + 1..];
    let end = rest.find("```")?;
    Some(rest[..end].trim())
}

fn parse_json_array(text: &str) -> Result<Vec<Value>> {
    if let Ok(arr) = serde_json::from_str::<Vec<Value>>(text) {
        if arr.is_empty() {
            bail!("empty action array");
        }
        return Ok(arr);
    }
    if let Ok(obj) = serde_json::from_str::<Value>(text) {
        if obj.is_object() && obj.get("action").is_some() {
            return Ok(vec![obj]);
        }
    }
    bail!("not a JSON array: {:?}", &text.chars().take(120).collect::<String>())
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
    let excerpt = file_lines[start_idx..end_idx]
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{}: {}", start_line + i, l))
        .collect::<Vec<_>>()
        .join("\n");
    Some((start_line, end_idx, excerpt))
}

/// Auto-read the region near the failed anchor, falling back to the full file.
fn auto_read_for_patch_anchor(workspace: &Path, relative: &str, err_msg: &str) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let full = std::fs::read_to_string(&path)
        .with_context(|| format!("auto-read failed: {}", path.display()))?;
    if let Some((start, end, excerpt)) = extract_anchor_context_excerpt(&full, err_msg) {
        return Ok(format!(
            "Current content near likely match of failed anchor in {relative} (lines {start}-{end}):\n{excerpt}"
        ));
    }
    // Fallback: first MAX_FULL_READ_LINES lines of the file.
    let text = full.lines()
        .take(MAX_FULL_READ_LINES)
        .enumerate()
        .map(|(i, l)| format!("{}: {}", i + 1, l))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("Current content of {relative}:\n{text}"))
}

// ── Action executors ───────────────────────────────────────────────────────────

fn exec_list_dir(workspace: &Path, relative: &str) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let mut entries = std::fs::read_dir(&path)
        .with_context(|| format!("list_dir: {}", path.display()))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries.join("\n"))
}

fn exec_read_file(workspace: &Path, relative: &str, start_line: Option<usize>) -> Result<String> {
    let path = safe_join(workspace, relative)?;
    let full = std::fs::read_to_string(&path)
        .with_context(|| format!("read_file: {}", path.display()))?;
    let lines: Vec<&str> = full.lines().collect();
    let total = lines.len();
    let (from, max_lines) = match start_line {
        Some(n) => (n.saturating_sub(1).min(total), 250),
        None => (0, MAX_FULL_READ_LINES),
    };
    let text = lines[from..]
        .iter()
        .take(max_lines)
        .enumerate()
        .map(|(i, l)| format!("{}: {}", from + i + 1, l))
        .collect::<Vec<_>>()
        .join("\n");
    let shown = max_lines.min(total.saturating_sub(from));
    if total > from + shown {
        Ok(format!("{text}\n(file has {total} lines total; use \"line\":{} to read more)", from + shown + 1))
    } else {
        Ok(text)
    }
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
    let output = Command::new("/bin/bash")
        .arg("-c")
        .arg(cmd)
        .current_dir(&cwd_path)
        .output()
        .with_context(|| format!("failed to spawn: {cmd}"))?;
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
    const BLOCKED: &[&str] =
        &["rm -rf", "git reset --hard", "git clean -f", "dd if=", "mkfs", "shred"];
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

// ── Agent loop ─────────────────────────────────────────────────────────────────

/// Run one agent role until it calls `done` or exhausts MAX_STEPS.
/// Returns the done reason on success, or an error on hard failure.
/// `check_on_done`: if true, run cargo build + test before accepting done.
async fn run_agent(
    role: &str,
    system_instructions: &str,
    initial_prompt: String,
    endpoint: &LlmEndpoint,
    bridge: &WsBridge,
    workspace: &Path,
    config: &CapabilityConfig,
    tabs: &TabManagerHandle,
    check_on_done: bool,
) -> Result<String> {
    let mut step = 0usize;
    let mut last_result: Option<String> = None;

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
            bridge, &endpoint.id, &endpoint.url, endpoint.stateful,
            &prompt, &role_schema, None, None, false, true,
            role, tabs, endpoint.max_tabs, config.tab_cooldown_ms,
        ).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[{role}] step={} llm_error: {e}", step + 1);
                last_result = Some(format!("LLM error: {e}\nReturn exactly one action in a ```json code block."));
                step += 1;
                continue;
            }
        };

        eprintln!("[{role}] step={} response_bytes={}", step + 1, raw.len());

        let actions = match parse_actions(&raw) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[{role}] step={} parse_error: {e}", step + 1);
                last_result = Some(format!("Parse error: {e}\nReturn exactly one action in a ```json code block. No prose outside it."));
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

        let action = &actions[0];
        let kind = action.get("action").and_then(|v| v.as_str()).unwrap_or("unknown");
        eprintln!("[{role}] step={} action={kind}", step + 1);

        let step_result: Result<(bool, String)> = (|| match kind {
            "done" => {
                let reason = action.get("reason").and_then(|v| v.as_str()).unwrap_or("complete");
                if !check_on_done {
                    return Ok((true, reason.to_string()));
                }
                eprintln!("[{role}] step={} done — running cargo build --workspace", step + 1);
                let (build_ok, build_out) = exec_run_command(workspace, "cargo build --workspace", WORKSPACE)
                    .unwrap_or_else(|e| (false, e.to_string()));
                if !build_ok {
                    eprintln!("[{role}] step={} cargo build failed — rejecting done", step + 1);
                    return Ok((false, format!(
                        "done rejected: cargo build --workspace failed.\n\n{}",
                        truncate(&build_out, MAX_SNIPPET)
                    )));
                }
                eprintln!("[{role}] step={} cargo build ok — running cargo test --workspace", step + 1);
                let (test_ok, test_out) = exec_run_command(workspace, "cargo test --workspace", WORKSPACE)
                    .unwrap_or_else(|e| (false, e.to_string()));
                if test_ok {
                    eprintln!("[{role}] step={} cargo test ok — accepting done", step + 1);
                    Ok((true, reason.to_string()))
                } else {
                    eprintln!("[{role}] step={} cargo test failed — rejecting done", step + 1);
                    Ok((false, format!(
                        "done rejected: cargo test --workspace failed.\n\n{}",
                        truncate(&test_out, MAX_SNIPPET)
                    )))
                }
            }
            "list_dir" => {
                let path = action.get("path").and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("list_dir missing 'path'"))?;
                let out = exec_list_dir(workspace, path)?;
                Ok((false, format!("list_dir {path}:\n{out}")))
            }
            "read_file" => {
                let path = action.get("path").and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("read_file missing 'path'"))?;
                let line = action.get("line").and_then(|v| v.as_u64()).map(|n| n as usize);
                let out = exec_read_file(workspace, path, line)?;
                eprintln!("[{role}] step={} read_file path={path} bytes={}", step + 1, out.len());
                Ok((false, format!("read_file {path}:\n{out}")))
            }
            "apply_patch" => {
                let patch = action.get("patch").and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("apply_patch missing 'patch'"))?;
                match apply_patch(patch, workspace) {
                    Ok(_) => {
                        eprintln!("[{role}] step={} apply_patch ok", step + 1);
                        let check_result = patch_first_file(patch)
                            .and_then(|f| infer_crate_for_patch(workspace, f))
                            .map(|krate| {
                                eprintln!("[{role}] step={} cargo check -p {krate}", step + 1);
                                exec_run_command(workspace, &format!("cargo check -p {krate}"), WORKSPACE)
                                    .unwrap_or_else(|e| (false, e.to_string()))
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
                        let mut msg = format!("apply_patch failed: {err_str}");
                        let read_path = extract_anchor_fail_path(&err_str)
                            .or_else(|| patch_first_file(patch).map(|s| s.to_string()));
                        if let Some(fp) = read_path {
                            if let Ok(content) = auto_read_for_patch_anchor(workspace, &fp, &err_str) {
                                eprintln!("[{role}] step={} auto_read path={fp}", step + 1);
                                msg = format!("apply_patch failed: {err_str}\n\n{content}");
                            }
                        }
                        Ok((false, msg))
                    }
                }
            }
            "run_command" => {
                let cmd = action.get("cmd").and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("run_command missing 'cmd'"))?;
                let cwd = action.get("cwd").and_then(|v| v.as_str()).unwrap_or(WORKSPACE);
                eprintln!("[{role}] step={} run_command cmd={cmd}", step + 1);
                let (success, out) = exec_run_command(workspace, cmd, cwd)?;
                let label = if success { "run_command ok" } else { "run_command failed" };
                eprintln!("[{role}] step={} {label} output_bytes={}", step + 1, out.len());
                Ok((false, format!("{label}:\n{}", truncate(&out, MAX_SNIPPET))))
            }
            other => Ok((false, format!(
                "unsupported action '{other}' — use list_dir, read_file, apply_patch, run_command, or done"
            ))),
        })();

        match step_result {
            Ok((true, reason)) => {
                eprintln!("[{role}] done: {reason}");
                return Ok(reason);
            }
            Ok((false, out)) => { last_result = Some(out); }
            Err(e) => {
                eprintln!("[{role}] step={} error: {e}", step + 1);
                last_result = Some(format!("Error executing action: {e}"));
            }
        }
        step += 1;
    }

}


fn find_endpoint<'a>(config: &'a CapabilityConfig, role: &str) -> Result<&'a LlmEndpoint> {
    config.llm_endpoints.iter()
        .find(|e| e.role.as_deref() == Some(role))
        .ok_or_else(|| anyhow!("no endpoint with role '{role}' in capability_config.toml"))
}

// ── Main ───────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let orchestrate = args.iter().any(|a| a == "--orchestrate");
    let start_role = args
        .windows(2)
        .find(|w| w[0] == "--start")
        .map(|w| w[1].as_str())
        .unwrap_or("executor");
    if !matches!(start_role, "executor" | "verifier" | "planner" | "intent" | "discovery") {
        bail!("invalid --start value: {start_role} (expected executor|verifier|planner|intent|discovery)");
    }
    let is_verifier  = !orchestrate && args.iter().any(|a| a == "--verifier");
    let is_planner   = !orchestrate && args.iter().any(|a| a == "--planner");
    let is_discovery = !orchestrate && args.iter().any(|a| a == "--discovery");
    let ws_port: u16 = args.windows(2)
        .find(|w| w[0] == "--port")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(WS_PORT_DEFAULT);

    let workspace = PathBuf::from(WORKSPACE);
    let plan_path = workspace.join(PLAN_FILE);

    let config = CapabilityConfig::snapshot_store_load()
        .context("failed to load capability_config.toml")?;

    let ws_addr: std::net::SocketAddr = format!("127.0.0.1:{ws_port}").parse()?;
    let bridge = ws_server::spawn(ws_addr, config.response_timeout_secs, Arc::new(OnceLock::new()));
    eprintln!("[canon-mini-agent] waiting for Chrome extension on ws://127.0.0.1:{ws_port}");
    bridge.wait_for_connection().await;
    eprintln!("[canon-mini-agent] Chrome extension connected");

    let tabs = llm_worker_new_tabs();

    if orchestrate {
        const MAX_CYCLES: usize = 10;
        eprintln!("[orchestrate] start_role={start_role}");
        for cycle in 0..MAX_CYCLES {
            eprintln!("[orchestrate] ── cycle {} ──────────────────────────────", cycle + 1);
            let order: [&str; 4] = match start_role {
                "verifier" => ["verifier", "planner", "executor", "intent"],
                "planner" => ["planner", "executor", "verifier", "intent"],
                "intent" => ["intent", "planner", "executor", "verifier"],
                _ => ["executor", "verifier", "planner", "intent"],
            };

            // ── Discovery phase (runs before all other agents every cycle) ──────
            {
                let ep = find_endpoint(&config, "discovery")?.clone();
                let plan = std::fs::read_to_string(&plan_path)
                    .with_context(|| format!("failed to read {PLAN_FILE}"))?;
                let prompt = format!(
                    "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\n\
                    Plan (from {PLAN_FILE}):\n{plan}\n\n\
                    Explore the workspace thoroughly and write a full discovery report to PLANS/discovery.md. \
                    Emit exactly one action to begin."
                );
                eprintln!("[orchestrate] cycle={} starting discovery", cycle + 1);
                let _ = run_agent("discovery", SYSTEM_INSTRUCTIONS_DISCOVERY, prompt, &ep, &bridge, &workspace, &config, &tabs, false).await;
            }

            let mut verify_result: Option<String> = None;

            for role in order {
                match role {
                    "intent" => {
                        let ep = find_endpoint(&config, "intent")?.clone();
                        let prompt = format!(
                            "Read PLANS/mini-agent-plan.md.\n\
Extract the TRUE objective.\n\
Write INTENT.md with EXACT structure:\n\n\
# INTENT\n\
## Objective\n<one sentence>\n\
## Constraints\n- no build break\n- no test failure\n\
## Targets\n<modules/files>\n\
## Success Criteria\n- verifiable conditions\n\n\
ONLY write INTENT.md using apply_patch.\n\
DO NOT modify any other file."
                        );
                        let _ = run_agent("intent", SYSTEM_INSTRUCTIONS_INTENT, prompt, &ep, &bridge, &workspace, &config, &tabs, false).await?;
                    }
                    "executor" => {
                        let plan = std::fs::read_to_string(&plan_path)
                            .with_context(|| format!("failed to read {PLAN_FILE}"))?;
                        let discovery = std::fs::read_to_string(workspace.join("PLANS/discovery.md")).unwrap_or_default();
                        let ep = find_endpoint(&config, "mini_agent")?.clone();
                        eprintln!("[orchestrate] starting executor");
                        let prompt = if discovery.is_empty() {
                            format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nObjective (from {PLAN_FILE}):\n{plan}\n\nEmit exactly one action to begin.")
                        } else {
                            format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nDiscovery report (from PLANS/discovery.md):\n{discovery}\n\nObjective (from {PLAN_FILE}):\n{plan}\n\nEmit exactly one action to begin.")
                        };
                        let _exec_result = run_agent("executor", SYSTEM_INSTRUCTIONS_EXECUTOR, prompt, &ep, &bridge, &workspace, &config, &tabs, false).await?;
                    }
                    "verifier" => {
                        let plan = std::fs::read_to_string(&plan_path)
                            .with_context(|| format!("failed to read {PLAN_FILE}"))?;
                        let ep = find_endpoint(&config, "verifier")?.clone();
                        eprintln!("[orchestrate] starting verifier");
                        let prompt = format!(
                            "WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nPlan to verify (from {PLAN_FILE}):\n{plan}\n\nBegin by reading the plan and identifying all tasks marked `- [x]`. Verify each one. Emit exactly one action to begin."
                        );
                        verify_result = Some(
                            run_agent("verifier", SYSTEM_INSTRUCTIONS_VERIFIER, prompt, &ep, &bridge, &workspace, &config, &tabs, false).await?
                        );
                    }
                    "planner" => {
                        let plan_after = std::fs::read_to_string(&plan_path)
                            .with_context(|| format!("failed to read {PLAN_FILE}"))?;
                        let intent = std::fs::read_to_string(workspace.join("INTENT.md")).unwrap_or_default();
                        let ep = find_endpoint(&config, "mini_planner")?.clone();
                        eprintln!("[orchestrate] starting planner");
                        let prompt = format!(
                            "WORKSPACE: {WORKSPACE}\n\nINTENT:\n{intent}\n\nPLAN:\n{plan_after}\n\nExpand all pending tasks."
                        );
                        let _plan_result = run_agent("planner", SYSTEM_INSTRUCTIONS_PLANNER, prompt, &ep, &bridge, &workspace, &config, &tabs, false).await?;
                    }
                    _ => unreachable!("validated start role"),
                }
            }

            let verify_result = verify_result.unwrap_or_else(|| "{\"verified\":false,\"summary\":\"verifier did not run\"}".to_string());
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
        } else if is_planner {
            ("mini_planner", SYSTEM_INSTRUCTIONS_PLANNER)
        } else if is_discovery {
            ("discovery", SYSTEM_INSTRUCTIONS_DISCOVERY)
        } else {
            ("mini_agent", SYSTEM_INSTRUCTIONS_EXECUTOR)
        };

        let plan = std::fs::read_to_string(&plan_path)
            .with_context(|| format!("failed to read {PLAN_FILE}"))?;
        if plan.trim().is_empty() {
            bail!("plan file is empty — write an objective into {PLAN_FILE} before running");
        }
        eprintln!("[canon-mini-agent] role={role} plan loaded ({} bytes)", plan.len());

        let endpoint = find_endpoint(&config, role)?.clone();
        eprintln!("[canon-mini-agent] endpoint id={} url={}", endpoint.id, endpoint.url);

        let initial_prompt = if is_verifier {
            format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nPlan to verify (from {PLAN_FILE}):\n{plan}\n\nBegin by reading the plan and identifying all tasks marked `- [x]`. Verify each one. Emit exactly one action to begin.")
        } else if is_planner {
            format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nCurrent plan (from {PLAN_FILE}):\n{plan}\n\nExpand all pending tasks (`- [ ]`) into concrete, actionable steps. Emit exactly one action to begin.")
        } else if is_discovery {
            format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nPlan (from {PLAN_FILE}):\n{plan}\n\nExplore the workspace thoroughly and write a full discovery report to PLANS/discovery.md. Emit exactly one action to begin.")
        } else {
            let discovery = std::fs::read_to_string(workspace.join("PLANS/discovery.md")).unwrap_or_default();
            if discovery.is_empty() {
                format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nObjective (from {PLAN_FILE}):\n{plan}\n\nEmit exactly one action to begin.")
            } else {
                format!("WORKSPACE: {WORKSPACE}\nAll relative paths resolve against WORKSPACE.\n\nDiscovery report (from PLANS/discovery.md):\n{discovery}\n\nObjective (from {PLAN_FILE}):\n{plan}\n\nEmit exactly one action to begin.")
            }
        };

        let reason = run_agent(role, instructions, initial_prompt, &endpoint, &bridge, &workspace, &config, &tabs, true).await?;
        println!("done: {reason}");
        Ok(())
    }
}

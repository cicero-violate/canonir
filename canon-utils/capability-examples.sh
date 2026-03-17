#!/usr/bin/env bash
# cap-examples.sh — hardcoded examples for every registered runtime capability
#
# How the API works:
#   Each capability is a capability_requested event written into the tlog.
#   canon-runtime watches the tlog, dispatches the event to the matching
#   CapabilityHandler, and writes a capability_completed (or capability_failed)
#   event back into the same tlog.
#
#   The emit_capability_event binary is the CLI entrypoint.
#   Flags:
#     --tlog <path>         tlog directory (default: $CANON_TLOG_PATH or state/event_log/event.tlog.d)
#     --name <capability>   registered capability name
#     --args <json>         JSON object passed as the args payload
#     --request-id <id>     optional; auto-generated if omitted
#
# Run canon-runtime first (in a separate terminal):
#   ./start.sh

set -euo pipefail

REPO="/workspace/ai_sandbox/canon"
TLOG="$REPO/state/event_log/event.tlog.d"
EMIT="cargo run -q -p canon-runtime-events --bin emit_capability_event --"

cd "$REPO"

emit() {
    local name="$1"
    local args="$2"
    echo ">>> $name"
    echo "    args: $args"
    $EMIT --tlog "$TLOG" --name "$name" --args "$args"
    echo ""
}

# ─────────────────────────────────────────────
# bash
#
# Runs a shell command in a login bash inside the workspace root.
# Args:
#   cmd  string   the shell command to run
#
# The result payload contains: { status, success, stdout, stderr }
# ─────────────────────────────────────────────

echo "=== bash ==="

# List files in workspace
emit "bash" '{"cmd": "ls -la /workspace/ai_sandbox/canon/canon-utils"}'

# Run a one-liner that touches real code
emit "bash" '{"cmd": "grep -r \"pub struct\" /workspace/ai_sandbox/canon/canon-utils/canon-planning/src/ --include=\"*.rs\" | head -10"}'

# Write a temp file and echo its content
emit "bash" '{"cmd": "echo hello-from-capability > /tmp/canon-test.txt && cat /tmp/canon-test.txt"}'

# Count lines in a source file
emit "bash" '{"cmd": "wc -l /workspace/ai_sandbox/canon/canon-utils/canon-runtime-events/src/events.rs"}'

# Run git log inside the repo
emit "bash" '{"cmd": "git -C /workspace/ai_sandbox/canon log --oneline -5"}'


# ─────────────────────────────────────────────
# cargo.build
#
# Builds a crate by name using cargo build.
# Args:
#   crate  string   Rust package name (as in Cargo.toml [package] name)
#
# The result payload contains: { status, success, stdout, stderr }
# ─────────────────────────────────────────────

echo "=== cargo.build ==="

emit "cargo.build" '{"crate": "canon-runtime-events"}'
emit "cargo.build" '{"crate": "canon-planning"}'
emit "cargo.build" '{"crate": "canon-runtime"}'


# ─────────────────────────────────────────────
# cargo.check
#
# Runs cargo check on a crate. Faster than build; validates types only.
# Args:
#   crate  string   Rust package name
# ─────────────────────────────────────────────

echo "=== cargo.check ==="

emit "cargo.check" '{"crate": "canon-tools-analysis"}'
emit "cargo.check" '{"crate": "canon-storage-graph"}'


# ─────────────────────────────────────────────
# file.read
#
# Reads a file from disk. Content returned in stdout field of result.
# Args:
#   path  string   absolute path to the file
# ─────────────────────────────────────────────

echo "=== file.read ==="

emit "file.read" '{"path": "/workspace/ai_sandbox/canon/canon-utils/canonical_items.md"}'
emit "file.read" '{"path": "/workspace/ai_sandbox/canon/canon-utils/canon-planning/src/capability_types.rs"}'


# ─────────────────────────────────────────────
# file.write
#
# Writes content to a file, creating it if it doesn't exist.
# Args:
#   path     string   absolute path to write
#   content  string   file content
# ─────────────────────────────────────────────

echo "=== file.write ==="

emit "file.write" '{
  "path": "/tmp/canon-cap-test/hello.txt",
  "content": "written by file.write capability\n"
}'

emit "file.write" '{
  "path": "/tmp/canon-cap-test/example.rs",
  "content": "fn greet(name: &str) -> String {\n    format!(\"hello, {name}\")\n}\n"
}'


# ─────────────────────────────────────────────
# edit.rename_symbol
#
# Renames a Rust symbol (function, struct, enum, etc.) across the project.
# The EditConsumer picks this up and calls canon-editor to apply the rename.
# Args:
#   project  string   path to the cargo workspace root
#   old      string   current symbol name
#   new      string   desired new name
# ─────────────────────────────────────────────

echo "=== edit.rename_symbol ==="

emit "edit.rename_symbol" '{
  "project": "/workspace/ai_sandbox/canon",
  "old": "GoalSpec",
  "new": "TaskSpec"
}'

emit "edit.rename_symbol" '{
  "project": "/workspace/ai_sandbox/canon",
  "old": "task_graph_resolve_ready",
  "new": "task_graph_advance_ready"
}'


# ─────────────────────────────────────────────
# edit.rename_module
#
# Renames a Rust module (file rename + all mod/use references updated).
# Args:
#   project  string   workspace root path
#   old      string   current module name (e.g. "decompose")
#   new      string   new module name (e.g. "task_decompose")
# ─────────────────────────────────────────────

echo "=== edit.rename_module ==="

emit "edit.rename_module" '{
  "project": "/workspace/ai_sandbox/canon",
  "old": "goal_embedding",
  "new": "task_embedding"
}'


# ─────────────────────────────────────────────
# edit.rename_dir
#
# Renames a directory and updates all Cargo.toml path references and use
# declarations that referenced the old path.
# Args:
#   project  string   workspace root path
#   old      string   old relative path from workspace root
#   new      string   new relative path
# ─────────────────────────────────────────────

echo "=== edit.rename_dir ==="

emit "edit.rename_dir" '{
  "project": "/workspace/ai_sandbox/canon",
  "old": "canon-utils/canon-planning",
  "new": "canon-utils/canon-goal-planning"
}'


# ─────────────────────────────────────────────
# edit.move_symbol
#
# Moves a symbol from its current module to a different module.
# Args:
#   project  string   workspace root path
#   symbol   string   symbol name to move
#   module   string   target module path (e.g. "canon_planning::task_graph")
# ─────────────────────────────────────────────

echo "=== edit.move_symbol ==="

emit "edit.move_symbol" '{
  "project": "/workspace/ai_sandbox/canon",
  "symbol": "NodeStatus",
  "module": "canon_planning::task_graph"
}'


# ─────────────────────────────────────────────
# apply_patch
#
# NOTE: apply_patch is a PipelineCapability used by the planning layer to
# classify tasks that write files. At runtime it does NOT have its own
# CapabilityHandler — tasks with ApplyPatch capability are dispatched as
# "bash" with the patch command embedded in the node description.
#
# The recommended approach is to use bash with `patch` or `git apply`:
# ─────────────────────────────────────────────

echo "=== apply_patch (via bash) ==="

# Write a patch file and apply it
emit "bash" '{
  "cmd": "cat > /tmp/canon-test.patch <<'"'"'PATCH'"'"'\n--- a/hello.txt\n+++ b/hello.txt\n@@ -1 +1 @@\n-written by file.write capability\n+patched by apply_patch example\nPATCH\ncp /tmp/canon-cap-test/hello.txt /tmp/hello-orig.txt && patch /tmp/hello-orig.txt /tmp/canon-test.patch && cat /tmp/hello-orig.txt"
}'

# Apply a unified diff using git apply (safer for source files)
emit "bash" '{
  "cmd": "cd /workspace/ai_sandbox/canon && git diff HEAD -- canon-utils/start.sh | head -20"
}'

cargo run -p canon-runtime-events --bin emit_capability_event -- \
  --tlog /workspace/ai_sandbox/canon/state/event_log/event.tlog.d \
  --capability llm.call \
  --args '{"prompt":"hello","raw":true}'


echo "done."

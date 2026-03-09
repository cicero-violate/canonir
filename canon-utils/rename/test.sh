RENAME_VALIDATE_UPG=1 cat <<'JSON' | cargo run --bin rename_stdin
{
  "project": "/workspace/ai_sandbox/canon/canon-agent-v2",
  "verify": false,
  "check": false,
  "ops": [
    { "op": "RenameModule", "args": { "old_module_path": "pipelines", "new_name": "pipelines_core" } }
  ]
}
JSON

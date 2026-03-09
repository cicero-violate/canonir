RENAME_VALIDATE_UPG=1 cat <<'JSON' | cargo run --bin rename_stdin
{
  "project": "/workspace/ai_sandbox/canon/canon-agent-v2",
  "verify": true,
  "check": true,
  "ops": [
    { "op": "RenameModule", "args": { "old_module_path": "pipelines_core_2", "new_name": "pipelines_core_3" } }
  ]
}
JSON

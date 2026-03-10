RENAME_VALIDATE_UPG=1 cat <<'JSON' | cargo run --bin project_editor
{
  "project": "/workspace/ai_sandbox/canon/canon-agent-v2",
  "verify": false,
  "check": false,
  "ops": [
    { "op": "RenameModule", "args": { "old_module_path": "pipelines_core", "new_name": "pipelines_core_3" } }
  ]
}
JSON

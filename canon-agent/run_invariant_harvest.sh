cargo run -p canon-agent -- run-invariant \
  /workspace/ai_sandbox/canon \
  /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap \
  /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap \
  /workspace/ai_sandbox/canon/target/debug/orchestration \
   5


cargo run -p canon-agent -- run-invariant \
  /workspace/ai_sandbox/canon \ (THIS IS FUCKING CALLED CWD)
  /workspace/ai_sandbox/canon/test_projects/test_rust_projects/capture/repomap \ (THIS IS FUCKIGN CALLED CAPTURE_DIRECTORY)
  /workspace/ai_sandbox/canon/test_projects/test_rust_projects/emit/repomap \ (THIS IS FUCKING CALLED EMIT_DIRECTORY)
  /workspace/ai_sandbox/canon/target/debug/orchestration \
   5

cd /workspace/ai_sandbox/canon_kernel
cargo build
cd /workspace/ai_sandbox/canon/state
cargo clean
rm -rf kernel_logs/*
cargo build

# CANON_REPORTS_PANIC_ON_CALLSITE_MISMATCH=1 \ # Panics when CALL edges exist but CALL_SITE nodes are 0.
# CANON_REPORTS_PANIC_ON_BLOCK_MISMATCH=1 \ # Panics when functions exist but HAS_BLOCK or FLOW is 0.
# CANON_REPORTS_PANIC_ON_NO_BRANCHES=1 \ # Panics when functions exist but no branch nodes (fan-out > 1) are detected.
# CANON_REPORTS_PANIC_ON_SPARSE_CALLGRAPH=1 \ # Panics when calls_per_function < 0.05.
cargo run \
  --manifest-path /workspace/ai_sandbox/canon/canon-utils/reports/Cargo.toml \
  --bin reports_from_tlog \
  -- \
  --tlog /workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog \
  --out /workspace/ai_sandbox/canon/state/kernel_logs/reports_out/kernel 

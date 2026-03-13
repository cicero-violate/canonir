cargo clean
rm -rf kernel_logs/*
cargo build
CANON_REPORTS_PANIC_ON_EMPTY_CFG=1 cargo run \
  --manifest-path /workspace/ai_sandbox/canon/canon-utils/reports/Cargo.toml \
  --bin reports_from_tlog \
  -- \
  --tlog /workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog \
  --out /workspace/ai_sandbox/canon/state/kernel_logs/reports_out/kernel 

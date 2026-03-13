rm -f /workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog*
rm -rf /workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog.d

rm -f /workspace/ai_sandbox/canon/state/kernel_logs/*
rm -rf /workspace/ai_sandbox/canon/state/kernel_logs/*

export CANON_TLOG_FORMAT=binary
export CANON_TLOG_DUAL_WRITE=1

cargo clean
cargo build

cargo run --bin verify_tlog_equivalence -- \
  --json /workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog \
  --binary /workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog.d

cargo run --bin verify_tlog_equivalence -- \
  --json /tmp/kernel.tlog \
  --binary /tmp/kernel.tlog.d \
  --stress

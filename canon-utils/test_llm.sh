# Kill the old instance
kill 215714

# Rebuild with updated code
cargo build -p canon-kernel --bin canon-kernel

# Restart watching the unified event log
cargo run -p canon-kernel --bin canon-kernel -- \
   --tlog /workspace/ai_sandbox/canon/state/event_log/event.tlog.d



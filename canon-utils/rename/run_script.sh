SPAN_COLLECTOR_OUT=/workspace/ai_sandbox/canon/canon-utils/rename/span_file/canon_span_file.jsonl \
RUSTC_WRAPPER=/workspace/ai_sandbox/canon/target/debug/canon-span-launcher \
CARGO_INCREMENTAL=0 \
rustup run nightly cargo check --manifest-path /workspace/ai_sandbox/canon/canon-agent/Cargo.toml

RENAME_OFFSET=0 RENAME_LIMIT=5 RENAME_MODE=incremental cargo run --example rename_self
# RENAME_OFFSET=0 RENAME_MODE=bulk cargo run --example rename_self

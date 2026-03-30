cargo run --bin canon-mini-agent -- --executor
cargo run --bin canon-mini-agent -- --verifier

cargo run -p canon-mini-agent --bin canon-mini-agent -- --orchestrate --start intent
cargo run -p canon-mini-agent --bin canon-mini-agent -- --orchestrate --start executor
cargo run -p canon-mini-agent --bin canon-mini-agent -- --orchestrate --start verifier
cargo run -p canon-mini-agent --bin canon-mini-agent -- --orchestrate --start planner

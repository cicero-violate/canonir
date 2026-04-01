cargo run --bin canon-mini-agent -- --executor
cargo run --bin canon-mini-agent -- --verifier

cargo run -p canon-mini-agent --bin canon-mini-agent -- --orchestrate --start diagnostics
cargo run -p canon-mini-agent --bin canon-mini-agent -- --orchestrate --start planner
cargo run -p canon-mini-agent --bin canon-mini-agent -- --orchestrate --start executor
cargo run -p canon-mini-agent --bin canon-mini-agent -- --orchestrate --start verifier

while true; do
  cargo run -p canon-mini-agent --bin canon-mini-agent -- --orchestrate --start planner
  echo "Process exited. Restarting..."
  sleep 1
done

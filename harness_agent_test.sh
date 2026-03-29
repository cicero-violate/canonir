cd /workspace/ai_sandbox/canon && \
cargo run -p canon-runtime --bin canon-harness-suite --   \
--workspace /workspace/ai_sandbox/canon \
--max-rounds 1000 \
--max-steps-per-test 30

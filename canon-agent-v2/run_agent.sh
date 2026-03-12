# cargo run -p canon-agent -- run-multi-dag \
#   /workspace/ai_sandbox/canon \
#   1000
RUSTBACKTRACE=1 cargo run -p canon-agent-v2 --features cuda -- run-capability \
  /workspace/ai_sandbox/canon  


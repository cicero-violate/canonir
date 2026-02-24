cargo run --manifest-path /home/cicero-arch-omen/ai_sandbox/canon/canon/Cargo.toml -- \
  import-dot memory.dot \
  --ir /home/cicero-arch-omen/ai_sandbox/canon/canon/tests/data/valid_ir.json \
  --goal sovereign_memory \
  --output-ir ./memory_ir.json \
  --materialize-dir ./out

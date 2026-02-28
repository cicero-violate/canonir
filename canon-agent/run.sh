if [ ! -f ir.json ]; then
  echo "[run] bootstrapping ir.json from repomap..."
  python3 scripts/gen_ir.py > ir.json
else
  echo "[run] ir.json exists — skipping gen_ir (preserving accumulated state)"
fi
cargo run -- run-agent ir.json layout.json graph.json .

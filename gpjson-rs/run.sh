#!/usr/bin/env bash
set -euo pipefail

FILE="/workspace/ai_sandbox/canon/canon-agent/frames/assembled.jsonl"

if [[ "${1:-}" == "" ]]; then
  echo "Usage: ./run.sh <jsonpath> [jsonpath...]" >&2
  echo "Examples:" >&2
  echo "  ./run.sh '$.id'" >&2
  echo "  ./run.sh '$.event.type'" >&2
  echo "  ./run.sh '$.events[0]'" >&2
  echo "  ./run.sh '$.events[0:3]'" >&2
  echo "  ./run.sh \"$.status[?(@ == 'ok')]\"" >&2
  echo "" >&2
  echo "Running a default multi-query demo (auto-picked from first JSON object)..." >&2
  QUERIES_JSON="$(python scripts/pick_queries.py "$FILE")"
  if [[ "$QUERIES_JSON" == "[]" || "$QUERIES_JSON" == "" ]]; then
    echo "No queries could be inferred from the file." >&2
    exit 1
  fi

  mapfile -t QUERIES < <(
    python - <<'PY' "$QUERIES_JSON"
import json,sys
qs=json.loads(sys.argv[1])
for q in qs:
    print(q)
PY
  )

  cargo run --example query -- "$FILE" "${QUERIES[@]}"
  exit 0
fi

cargo run --example query -- "$FILE" "$@"

#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <crate> <test>" >&2
  exit 2
fi

crate="$1"
test_name="$2"

cd /workspace/ai_sandbox/canon
exec cargo test -p "$crate" "$test_name" -- --exact --nocapture

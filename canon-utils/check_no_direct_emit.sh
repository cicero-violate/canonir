#!/usr/bin/env bash
set -euo pipefail

# CI guard: ensure no direct CanonEvent emissions outside macro definitions.
rg --type rust '\.emit\(CanonEvent::' \
  canon-utils \
  --glob '!**/canon-macros/**' \
  --glob '!**/canon-runtime-events/**' \
  && { echo "ERROR: direct CanonEvent emits found"; exit 1; } \
  || true

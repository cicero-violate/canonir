#!/usr/bin/env bash
set -euo pipefail

KERNEL_WRAPPER="/workspace/ai_sandbox/canon_kernel/target/debug/canon_kernel"

if [[ ! -x "${KERNEL_WRAPPER}" ]]; then
  echo "canon_rustc_wrapper: kernel wrapper not found: ${KERNEL_WRAPPER}" >&2
  exit 2
fi

"${KERNEL_WRAPPER}" "$@"
status=$?

if [[ $status -ne 0 ]]; then
  exit $status
fi

exit 0

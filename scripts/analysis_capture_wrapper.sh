#!/usr/bin/env bash
set -euo pipefail

# Gate: auto-build analysis_capture if missing or forced, then delegate.

# Prevent infinite recursion during bootstrap.
if [[ "${ANALYSIS_CAPTURE_BUILDING:-}" == "1" ]]; then
  exec "$1" "${@:2}"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ANALYSIS_CAPTURE_BIN="$WORKSPACE_ROOT/target/debug/analysis_capture"
ANALYSIS_CAPTURE_SRC="$WORKSPACE_ROOT/canon-utils/rustc_wrapper/driver/src"
ANALYSIS_CAPTURE_MANIFEST="$WORKSPACE_ROOT/canon-utils/rustc_wrapper/driver/Cargo.toml"
UPG_ANALYSIS_SRC="$WORKSPACE_ROOT/canon-utils/upg_analysis/src"
UPG_ANALYSIS_MANIFEST="$WORKSPACE_ROOT/canon-utils/upg_analysis/Cargo.toml"

# If we are inside an active cargo session, cargo already holds the artifact
# lock. Never call `cargo build` here — it will deadlock. Delegate directly.
if [[ -n "${CARGO_PKG_NAME:-}" ]]; then
  if [[ -x "$ANALYSIS_CAPTURE_BIN" ]]; then
    exec "$ANALYSIS_CAPTURE_BIN" "$@"
  else
    exec "$1" "${@:2}"
  fi
fi

# Fast path for rustc probe invocations.
for arg in "${@:2}"; do
  case "$arg" in
    --print=*|-vV|--version)
      exec "$1" "${@:2}"
      ;;
  esac
done

NEED_BUILD=0
if [[ "${ANALYSIS_CAPTURE_FORCE_REFRESH:-}" == "1" || ! -x "$ANALYSIS_CAPTURE_BIN" ]]; then
  NEED_BUILD=1
else
  if find "$ANALYSIS_CAPTURE_SRC" "$UPG_ANALYSIS_SRC" \
    "$ANALYSIS_CAPTURE_MANIFEST" "$UPG_ANALYSIS_MANIFEST" \
    -type f -newer "$ANALYSIS_CAPTURE_BIN" -print -quit 2>/dev/null | grep -q .; then
    NEED_BUILD=1
  fi
fi

if [[ "$NEED_BUILD" == "1" ]]; then
  export ANALYSIS_CAPTURE_BUILDING=1
  (cd "$WORKSPACE_ROOT" && \
    RUSTC_WRAPPER= \
    RUSTC_WORKSPACE_WRAPPER= \
    CARGO_BUILD_RUSTC_WRAPPER= \
    cargo build -p analysis_capture)
  unset ANALYSIS_CAPTURE_BUILDING
fi

# Ensure rustc private libs are visible to analysis_capture.
RUSTC_BIN="${1:-}"
if [[ -z "$RUSTC_BIN" ]]; then
  RUSTC_BIN="$(command -v rustc 2>/dev/null || true)"
fi
if [[ -n "$RUSTC_BIN" && -x "$RUSTC_BIN" ]]; then
  SYSROOT="$("$RUSTC_BIN" --print=sysroot 2>/dev/null || true)"
  HOST="$("$RUSTC_BIN" -vV 2>/dev/null | awk '/^host:/{print $2; exit}')"
  if [[ -n "$SYSROOT" ]]; then
    LIB1="$SYSROOT/lib"
    if [[ -n "$HOST" ]]; then
      LIB2="$SYSROOT/lib/rustlib/$HOST/lib"
      export LD_LIBRARY_PATH="$LIB1:$LIB2:${LD_LIBRARY_PATH:-}"
    else
      export LD_LIBRARY_PATH="$LIB1:${LD_LIBRARY_PATH:-}"
    fi
  fi
fi

# Now delegate to analysis_capture (which should exist).
if [[ -x "$ANALYSIS_CAPTURE_BIN" ]]; then
  exec "$ANALYSIS_CAPTURE_BIN" "$@"
fi

# Fallback: use real rustc directly (only when called via RUSTC_WRAPPER).
if [[ $# -ge 1 && -n "${1:-}" ]]; then
  exec "$1" "${@:2}"
fi

echo "analysis_capture_wrapper: missing analysis_capture and no rustc fallback" >&2
exit 1

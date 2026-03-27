#!/usr/bin/env bash
set -euo pipefail

ROOT="/workspace/ai_sandbox/canon"
STDERR_CACHE="$ROOT/state/harness_repair.stderr"
SUPERVISOR_LOG="$ROOT/state/harness_repair_supervisor.log"

usage() {
  cat <<'EOF'
Usage:
  scripts/run_harness_repair.sh <crate> <test-name> [stderr-file|-] [--no-supervisor] [--always-dispatch]

Examples:
  scripts/run_harness_repair.sh canon-route \
    policy::tests::apply_route_policy_forces_plan_when_validation_is_precondition_blocked \
    /tmp/failure.txt

  cargo test -p canon-route policy::tests::foo -- --nocapture 2>&1 | \
    scripts/run_harness_repair.sh canon-route policy::tests::foo -

Behavior:
  - starts canon-runtime-supervisor if it is not already running
  - emits a direct RequestDispatch into the tlog using canon-harness-repair
  - if no stderr source is provided, the harness binary runs the target test itself
EOF
}

if [[ $# -lt 2 ]]; then
  usage
  exit 1
fi

CRATE_NAME="$1"
TEST_NAME="$2"
STDERR_INPUT=""
shift 2

if [[ $# -gt 0 && "$1" != --* ]]; then
  STDERR_INPUT="$1"
  shift
fi

USE_SUPERVISOR=1
ALWAYS_DISPATCH=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-supervisor)
      USE_SUPERVISOR=0
      ;;
    --always-dispatch)
      ALWAYS_DISPATCH=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
  shift
done

mkdir -p "$ROOT/state"

if [[ -n "$STDERR_INPUT" ]]; then
  if [[ "$STDERR_INPUT" == "-" ]]; then
    cat > "$STDERR_CACHE"
  else
    cp "$STDERR_INPUT" "$STDERR_CACHE"
  fi
fi

cd "$ROOT"
cargo build --bin canon-runtime --bin canon-runtime-supervisor --bin canon-harness-repair

if [[ "$USE_SUPERVISOR" -eq 1 ]]; then
  if ! pgrep -f "$ROOT/target/debug/canon-runtime-supervisor" >/dev/null 2>&1; then
    nohup "$ROOT/target/debug/canon-runtime-supervisor" >"$SUPERVISOR_LOG" 2>&1 &
    sleep 1
  fi
fi

HARNESS_ARGS=("$CRATE_NAME" "$TEST_NAME" "--workspace" "$ROOT" "--tlog" "$ROOT/state/event_log/event.tlog.d")
if [[ "$ALWAYS_DISPATCH" -eq 1 ]]; then
  HARNESS_ARGS+=("--always-dispatch")
fi
if [[ -n "$STDERR_INPUT" ]]; then
  HARNESS_ARGS+=("--stderr-file" "$STDERR_CACHE")
fi

exec "$ROOT/target/debug/canon-harness-repair" "${HARNESS_ARGS[@]}"

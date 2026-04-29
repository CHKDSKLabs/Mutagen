#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF' >&2
Usage:
  dev_console.sh --workspace-root <path> [dashboard_dev.sh options...]

This is the top-level local dev entrypoint for the Mutagen harness console.
It runs the deployment doctor first, then launches the dashboard.
EOF
  exit 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCTOR_SCRIPT="$SCRIPT_DIR/doctor_dev.sh"
DASHBOARD_SCRIPT="$SCRIPT_DIR/dashboard_dev.sh"
WORKSPACE_ROOT=""
ARGS=("$@")

if [[ $# -eq 0 ]]; then
  usage
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace-root)
      [[ $# -ge 2 ]] || usage
      WORKSPACE_ROOT="$2"
      shift 2
      ;;
    --help|-h)
      usage
      ;;
    *)
      shift
      ;;
  esac
done

if [[ -z "$WORKSPACE_ROOT" && -n "${MUTAGEN_WORKSPACE_ROOT:-}" ]]; then
  WORKSPACE_ROOT="$MUTAGEN_WORKSPACE_ROOT"
fi

if [[ -z "$WORKSPACE_ROOT" ]]; then
  printf 'dev_console.sh needs --workspace-root or MUTAGEN_WORKSPACE_ROOT\n' >&2
  exit 1
fi

bash "$DOCTOR_SCRIPT" --workspace-root "$WORKSPACE_ROOT"
printf '\n'
printf 'doctor finished. Launching the dashboard before the moment gets weird.\n'
printf '\n'
exec bash "$DASHBOARD_SCRIPT" "${ARGS[@]}"

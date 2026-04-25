#!/usr/bin/env bash

set -euo pipefail

HARNESS_ARGS=()


resolve_jq() {
  if command -v jq >/dev/null 2>&1; then
    command -v jq
    return 0
  fi

  if command -v jq.exe >/dev/null 2>&1; then
    command -v jq.exe
    return 0
  fi

  return 1
}

emit_error() {
  local error="$1"
  local message="$2"
  printf '{"ok":false,"error":"%s","message":"%s"}\n' "$error" "$message"
  exit 1
}

usage() {
  cat <<'EOF' >&2
Usage:
  materialize_cohort_worktrees.sh
    --workspace-root PATH
    --slice-id ID
    [--slice-id ID...]
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace-root|--slice-id)
      [[ $# -ge 2 ]] || usage
      HARNESS_ARGS+=("$1" "$2")
      shift 2
      ;;
    --help|-h)
      usage
      ;;
    *)
      usage
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JQ_BIN="$(resolve_jq)" || emit_error "materialize_cohort_worktrees_unavailable" "jq not found on PATH"

set +e
OUTPUT="$(
  bash "$SCRIPT_DIR/harness_runtime.sh" materialize-cohort-worktrees "${HARNESS_ARGS[@]}" 2>&1
)"
STATUS=$?
set -e

if [[ $STATUS -ne 0 ]]; then
  emit_error "materialize_cohort_worktrees_runtime_failure" "mutagen harness materialize-cohort-worktrees runtime failed"
fi

if ! printf '%s' "$OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "materialize_cohort_worktrees_runtime_failure" "mutagen harness materialize-cohort-worktrees returned non-JSON output"
fi

printf '%s\n' "$OUTPUT"

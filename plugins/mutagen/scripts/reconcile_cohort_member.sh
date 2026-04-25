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
  reconcile_cohort_member.sh
    --workspace-root PATH
    --worktree-root PATH
    --slice-id ID
    --run-output PATH
    [--merged-path-owner PATH=SLICE_ID]
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace-root|--worktree-root|--slice-id|--run-output|--merged-path-owner)
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
JQ_BIN="$(resolve_jq)" || emit_error "reconcile_cohort_member_unavailable" "jq not found on PATH"

set +e
OUTPUT="$(
  bash "$SCRIPT_DIR/harness_runtime.sh" reconcile-cohort-member "${HARNESS_ARGS[@]}" 2>&1
)"
STATUS=$?
set -e

if [[ $STATUS -ne 0 ]]; then
  emit_error "reconcile_cohort_member_runtime_failure" "mutagen harness reconcile-cohort-member runtime failed"
fi

if ! printf '%s' "$OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "reconcile_cohort_member_runtime_failure" "mutagen harness reconcile-cohort-member returned non-JSON output"
fi

printf '%s\n' "$OUTPUT"

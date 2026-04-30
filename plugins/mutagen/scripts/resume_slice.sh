#!/usr/bin/env bash
# Force-reset the active slice to the given slice/stage. Use this to unstick the
# pipeline when the active-slice.json is wedged or pointing at the wrong slice.

set -euo pipefail

WORKSPACE_ROOT="."
QUEUE_PATH="slices/queue.json"
WORKFLOW_CONFIG_PATH=".claude/workflow.json"
ACTIVE_STATE_PATH=".mutagen/state/active-slice.json"
HOST_KIND="codex"
SLICE_ID=""
FROM_STAGE="author"

usage() {
  cat <<'EOF' >&2
Usage:
  resume_slice.sh --slice-id ID [--from-stage STAGE] [--workspace-root PATH]
                  [--queue PATH] [--workflow-config PATH] [--active-state PATH]
                  [--host HOST]

Stages: author, structural_check, review, state_record (default: author)
EOF
  exit 1
}

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

absolute_path() {
  local path="$1"

  if [[ "$path" == /* ]]; then
    printf '%s\n' "$path"
    return 0
  fi

  printf '%s/%s\n' "$(pwd)" "$path"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace-root) [[ $# -ge 2 ]] || usage; WORKSPACE_ROOT="$2"; shift 2 ;;
    --queue) [[ $# -ge 2 ]] || usage; QUEUE_PATH="$2"; shift 2 ;;
    --workflow-config) [[ $# -ge 2 ]] || usage; WORKFLOW_CONFIG_PATH="$2"; shift 2 ;;
    --active-state) [[ $# -ge 2 ]] || usage; ACTIVE_STATE_PATH="$2"; shift 2 ;;
    --slice-id) [[ $# -ge 2 ]] || usage; SLICE_ID="$2"; shift 2 ;;
    --from-stage) [[ $# -ge 2 ]] || usage; FROM_STAGE="$2"; shift 2 ;;
    --host) [[ $# -ge 2 ]] || usage; HOST_KIND="$2"; shift 2 ;;
    --help|-h) usage ;;
    *) usage ;;
  esac
done

[[ -n "$SLICE_ID" ]] || usage

JQ_BIN="$(resolve_jq)" || {
  printf '{"ok":false,"reason":"tooling_failure","message":"jq not found on PATH"}\n'
  exit 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(absolute_path "$WORKSPACE_ROOT")"
QUEUE_PATH="$(absolute_path "$QUEUE_PATH")"
WORKFLOW_CONFIG_PATH="$(absolute_path "$WORKFLOW_CONFIG_PATH")"
ACTIVE_STATE_PATH="$(absolute_path "$ACTIVE_STATE_PATH")"

set +e
RESUME_OUTPUT="$(
  bash "$SCRIPT_DIR/harness_runtime.sh" resume-slice \
    --workspace-root "$WORKSPACE_ROOT" \
    --queue "$QUEUE_PATH" \
    --workflow-config "$WORKFLOW_CONFIG_PATH" \
    --active-state "$ACTIVE_STATE_PATH" \
    --slice-id "$SLICE_ID" \
    --from-stage "$FROM_STAGE" \
    --host "$HOST_KIND" 2>&1
)"
RESUME_STATUS=$?
set -e

if [[ $RESUME_STATUS -ne 0 ]]; then
  detail="$(printf '%s' "$RESUME_OUTPUT" | "$JQ_BIN" -Rsa . 2>/dev/null || printf '""')"
  printf '{"ok":false,"reason":"resume_slice_runtime_failure","slice_id":"%s","exit_code":%d,"message":%s}\n' \
    "$SLICE_ID" "$RESUME_STATUS" "$detail"
  exit 1
fi

if ! printf '%s' "$RESUME_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  detail="$(printf '%s' "$RESUME_OUTPUT" | "$JQ_BIN" -Rsa . 2>/dev/null || printf '""')"
  printf '{"ok":false,"reason":"resume_slice_runtime_failure","slice_id":"%s","message":%s}\n' \
    "$SLICE_ID" "$detail"
  exit 1
fi

printf '%s\n' "$RESUME_OUTPUT"

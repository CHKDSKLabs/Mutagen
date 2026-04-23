#!/usr/bin/env bash

set -euo pipefail

WORKSPACE_ROOT="."
QUEUE_PATH="slices/queue.json"
WORKFLOW_CONFIG_PATH=".claude/workflow.json"
ACTIVE_STATE_PATH=".mutagen/state/active-slice.json"
HOST_KIND="codex"
SLICE_ID=""
DRY_RUN=0

usage() {
  cat <<'EOF' >&2
Usage:
  prepare_selected_slice.sh --slice-id ID [--workspace-root PATH] [--queue PATH]
                            [--workflow-config PATH] [--active-state PATH]
                            [--host HOST] [--dry-run]
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

resolve_cargo() {
  if command -v cargo >/dev/null 2>&1; then
    command -v cargo
    return 0
  fi

  if [ -x "$HOME/.cargo/bin/cargo" ]; then
    printf '%s\n' "$HOME/.cargo/bin/cargo"
    return 0
  fi

  if command -v cargo.exe >/dev/null 2>&1; then
    command -v cargo.exe
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
    --workspace-root)
      [[ $# -ge 2 ]] || usage
      WORKSPACE_ROOT="$2"
      shift 2
      ;;
    --queue)
      [[ $# -ge 2 ]] || usage
      QUEUE_PATH="$2"
      shift 2
      ;;
    --workflow-config)
      [[ $# -ge 2 ]] || usage
      WORKFLOW_CONFIG_PATH="$2"
      shift 2
      ;;
    --active-state)
      [[ $# -ge 2 ]] || usage
      ACTIVE_STATE_PATH="$2"
      shift 2
      ;;
    --slice-id)
      [[ $# -ge 2 ]] || usage
      SLICE_ID="$2"
      shift 2
      ;;
    --host)
      [[ $# -ge 2 ]] || usage
      HOST_KIND="$2"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --help|-h)
      usage
      ;;
    *)
      usage
      ;;
  esac
done

[[ -n "$SLICE_ID" ]] || usage

JQ_BIN="$(resolve_jq)" || {
  printf '{"ok":false,"reason":"tooling_failure","message":"jq not found on PATH"}\n'
  exit 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/harness/Cargo.toml"

if [[ ! -f "$MANIFEST_PATH" ]]; then
  "$JQ_BIN" -n \
    --arg manifest "$MANIFEST_PATH" \
    '{
      ok: false,
      reason: "prepare_selected_slice_unavailable",
      message: ("mutagen harness manifest not found at " + $manifest)
    }'
  exit 1
fi

CARGO_BIN="$(resolve_cargo)" || {
  "$JQ_BIN" -n \
    --arg slice_id "$SLICE_ID" \
    '{
      ok: false,
      reason: "prepare_selected_slice_unavailable",
      slice_id: $slice_id,
      message: "cargo not found on PATH"
    }'
  exit 1
}

WORKSPACE_ROOT="$(absolute_path "$WORKSPACE_ROOT")"
QUEUE_PATH="$(absolute_path "$QUEUE_PATH")"
WORKFLOW_CONFIG_PATH="$(absolute_path "$WORKFLOW_CONFIG_PATH")"
ACTIVE_STATE_PATH="$(absolute_path "$ACTIVE_STATE_PATH")"

prepare_args=(
  "$CARGO_BIN" run
  --quiet
  --manifest-path "$MANIFEST_PATH"
  --
  prepare-selected-slice
  --workspace-root "$WORKSPACE_ROOT"
  --queue "$QUEUE_PATH"
  --workflow-config "$WORKFLOW_CONFIG_PATH"
  --active-state "$ACTIVE_STATE_PATH"
  --slice-id "$SLICE_ID"
  --host "$HOST_KIND"
)

if [[ $DRY_RUN -eq 1 ]]; then
  prepare_args+=(--dry-run)
fi

set +e
PREPARE_OUTPUT="$("${prepare_args[@]}" 2>&1)"
PREPARE_STATUS=$?
set -e

if [[ $PREPARE_STATUS -ne 0 ]]; then
  printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -Rs \
    --arg slice_id "$SLICE_ID" \
    --arg workspace_root "$WORKSPACE_ROOT" \
    --arg queue "$QUEUE_PATH" \
    --arg workflow_config "$WORKFLOW_CONFIG_PATH" \
    --arg active_state "$ACTIVE_STATE_PATH" \
    --arg host "$HOST_KIND" \
    '{
      ok: false,
      reason: "prepare_selected_slice_runtime_failure",
      slice_id: $slice_id,
      workspace_root: $workspace_root,
      queue: $queue,
      workflow_config: $workflow_config,
      active_state: $active_state,
      host: $host,
      message: .
    }'
  exit 1
fi

if ! printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -Rs \
    --arg slice_id "$SLICE_ID" \
    --arg workspace_root "$WORKSPACE_ROOT" \
    --arg queue "$QUEUE_PATH" \
    --arg workflow_config "$WORKFLOW_CONFIG_PATH" \
    --arg active_state "$ACTIVE_STATE_PATH" \
    --arg host "$HOST_KIND" \
    '{
      ok: false,
      reason: "prepare_selected_slice_runtime_failure",
      slice_id: $slice_id,
      workspace_root: $workspace_root,
      queue: $queue,
      workflow_config: $workflow_config,
      active_state: $active_state,
      host: $host,
      message: ("prepare-selected-slice returned non-JSON output: " + .)
    }'
  exit 1
fi

PREPARE_RESULT_STATUS="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.status')"

case "$PREPARE_RESULT_STATUS" in
  ready)
    printf '%s\n' "$PREPARE_OUTPUT"
    ;;
  blocked)
    printf '%s\n' "$PREPARE_OUTPUT"
    exit 2
    ;;
  *)
    "$JQ_BIN" -n \
      --arg status "$PREPARE_RESULT_STATUS" \
      --arg slice_id "$SLICE_ID" \
      '{
        ok: false,
        reason: "prepare_selected_slice_runtime_failure",
        slice_id: $slice_id,
        message: ("prepare-selected-slice returned unsupported status `" + $status + "`")
      }'
    exit 1
    ;;
esac

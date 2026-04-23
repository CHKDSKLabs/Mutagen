#!/usr/bin/env bash
# Select and claim the next ready slice through the Rust harness.
#
# Usage:
#   prepare_next.sh [--workspace-root PATH] [--queue PATH]
#                   [--queue-validation PATH] [--workflow-config PATH]
#                   [--active-state PATH] [--host HOST] [--dry-run]
#
# Exit codes:
#   0 = prepare-next completed (ready, queue_clear, or stalled)
#   1 = helper/runtime failure
#   2 = queue is not ready for execution

set -euo pipefail

WORKSPACE_ROOT="."
QUEUE_PATH="slices/queue.json"
QUEUE_VALIDATION_PATH=".mutagen/state/queue-validation.json"
WORKFLOW_CONFIG_PATH=".claude/workflow.json"
ACTIVE_STATE_PATH=".mutagen/state/active-slice.json"
HOST_KIND="codex"
DRY_RUN=0

usage() {
  cat <<'EOF' >&2
Usage:
  prepare_next.sh [--workspace-root PATH] [--queue PATH]
                  [--queue-validation PATH] [--workflow-config PATH]
                  [--active-state PATH] [--host HOST] [--dry-run]
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
    --queue-validation)
      [[ $# -ge 2 ]] || usage
      QUEUE_VALIDATION_PATH="$2"
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
      reason: "prepare_next_unavailable",
      message: ("mutagen harness manifest not found at " + $manifest)
    }'
  exit 1
fi

CARGO_BIN="$(resolve_cargo)" || {
  "$JQ_BIN" -n \
    --arg workspace_root "$WORKSPACE_ROOT" \
    --arg queue "$QUEUE_PATH" \
    --arg workflow_config "$WORKFLOW_CONFIG_PATH" \
    --arg active_state "$ACTIVE_STATE_PATH" \
    --arg host "$HOST_KIND" \
    '{
      ok: false,
      reason: "prepare_next_unavailable",
      workspace_root: $workspace_root,
      queue: $queue,
      workflow_config: $workflow_config,
      active_state: $active_state,
      host: $host,
      message: "cargo not found on PATH"
    }'
  exit 1
}

WORKSPACE_ROOT="$(absolute_path "$WORKSPACE_ROOT")"
QUEUE_PATH="$(absolute_path "$QUEUE_PATH")"
QUEUE_VALIDATION_PATH="$(absolute_path "$QUEUE_VALIDATION_PATH")"
WORKFLOW_CONFIG_PATH="$(absolute_path "$WORKFLOW_CONFIG_PATH")"
ACTIVE_STATE_PATH="$(absolute_path "$ACTIVE_STATE_PATH")"

set +e
QUEUE_READY_OUTPUT="$(
  "$SCRIPT_DIR/queue_ready.sh" "$QUEUE_PATH" "$QUEUE_VALIDATION_PATH" 2>&1
)"
QUEUE_READY_STATUS=$?
set -e

if [[ $QUEUE_READY_STATUS -ne 0 ]]; then
  printf '%s\n' "$QUEUE_READY_OUTPUT"
  exit "$QUEUE_READY_STATUS"
fi

prepare_args=(
  "$CARGO_BIN" run
  --quiet
  --manifest-path "$MANIFEST_PATH"
  --
  prepare-next
  --workspace-root "$WORKSPACE_ROOT"
  --queue "$QUEUE_PATH"
  --workflow-config "$WORKFLOW_CONFIG_PATH"
  --active-state "$ACTIVE_STATE_PATH"
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
    --arg workspace_root "$WORKSPACE_ROOT" \
    --arg queue "$QUEUE_PATH" \
    --arg workflow_config "$WORKFLOW_CONFIG_PATH" \
    --arg active_state "$ACTIVE_STATE_PATH" \
    --arg host "$HOST_KIND" \
    '{
      ok: false,
      reason: "prepare_next_runtime_failure",
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
    --arg workspace_root "$WORKSPACE_ROOT" \
    --arg queue "$QUEUE_PATH" \
    --arg workflow_config "$WORKFLOW_CONFIG_PATH" \
    --arg active_state "$ACTIVE_STATE_PATH" \
    --arg host "$HOST_KIND" \
    '{
      ok: false,
      reason: "prepare_next_runtime_failure",
      workspace_root: $workspace_root,
      queue: $queue,
      workflow_config: $workflow_config,
      active_state: $active_state,
      host: $host,
      message: ("prepare-next returned non-JSON output: " + .)
    }'
  exit 1
fi

if [[ $DRY_RUN -eq 0 ]]; then
  printf '%s\n' "$PREPARE_OUTPUT" | "$SCRIPT_DIR/emit_notifications.sh" >/dev/null 2>&1 || true
fi

printf '%s\n' "$PREPARE_OUTPUT"

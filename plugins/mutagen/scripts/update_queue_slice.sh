#!/usr/bin/env bash

set -euo pipefail

QUEUE_PATH="slices/queue.json"
SLICEMAP_PATH="slices/slicemap.md"
LEGACY_PATH="slices/queue.md"
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
  update_queue_slice.sh [--queue PATH] [--slicemap PATH] [--legacy PATH]
    --slice-id ID
    [--status STATUS]
    [--attempts N]
    [--micro-corrections-used N]
    [--karai-structural pass|fail]
    [--bishop clean|advisory|block|skip]
    [--tiger-claw clean|gap|defect|skip]
    [--micro-correction true|false]
    [--completed-at ISO-8601]
    [--clear-completed-at]
    [--escalation-reason TEXT]
    [--clear-escalation-reason]
    [--human-check-resolved-at ISO-8601]
    [--resolve-human-check]
    [--clear-human-check-resolved-at]

Status values: pending, in_progress, blocked_retry, completed, escalated,
refused. Clap accepts the kebab-case form (e.g. `in-progress`) too because it
is a derived ValueEnum, but the on-disk queue stores snake_case. Prefer the
snake_case form to keep CLI input and queue contents consistent.
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --queue)
      [[ $# -ge 2 ]] || usage
      QUEUE_PATH="$2"
      HARNESS_ARGS+=("$1" "$2")
      shift 2
      ;;
    --slicemap)
      [[ $# -ge 2 ]] || usage
      SLICEMAP_PATH="$2"
      shift 2
      ;;
    --legacy)
      [[ $# -ge 2 ]] || usage
      LEGACY_PATH="$2"
      shift 2
      ;;
    --slice-id|--status|--attempts|--micro-corrections-used|--karai-structural|--bishop|--tiger-claw|--micro-correction|--completed-at|--escalation-reason|--human-check-resolved-at)
      [[ $# -ge 2 ]] || usage
      HARNESS_ARGS+=("$1" "$2")
      shift 2
      ;;
    --clear-completed-at|--clear-escalation-reason|--clear-human-check-resolved-at|--resolve-human-check)
      HARNESS_ARGS+=("$1")
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

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JQ_BIN="$(resolve_jq)" || emit_error "update_slice_unavailable" "jq not found on PATH"

set +e
OUTPUT="$(
  bash "$SCRIPT_DIR/harness_runtime.sh" update-slice "${HARNESS_ARGS[@]}" 2>&1
)"
STATUS=$?
set -e

if [[ $STATUS -ne 0 ]]; then
  # If the harness already produced structured JSON on its way out the door, forward it so
  # the orchestrator sees the real error instead of our generic wrapper.
  if printf '%s' "$OUTPUT" | "$JQ_BIN" -e 'type == "object"' >/dev/null 2>&1; then
    printf '%s\n' "$OUTPUT"
    exit 1
  fi
  DETAIL="$(printf '%s' "$OUTPUT" | "$JQ_BIN" -Rsa . 2>/dev/null || printf '""')"
  printf '{"ok":false,"error":"update_slice_runtime_failure","message":"mutagen harness update-slice exited %d","detail":%s}\n' "$STATUS" "$DETAIL"
  exit 1
fi

if ! printf '%s' "$OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  DETAIL="$(printf '%s' "$OUTPUT" | "$JQ_BIN" -Rsa . 2>/dev/null || printf '""')"
  printf '{"ok":false,"error":"update_slice_runtime_failure","message":"mutagen harness update-slice returned non-JSON output","detail":%s}\n' "$DETAIL"
  exit 1
fi

set +e
RENDER_OUTPUT="$(
  "$SCRIPT_DIR/render_queue.sh" "$QUEUE_PATH" "$SLICEMAP_PATH" "$LEGACY_PATH" 2>&1
)"
RENDER_STATUS=$?
set -e

if [[ $RENDER_STATUS -ne 0 ]]; then
  DETAIL="$(printf '%s' "$RENDER_OUTPUT" | "$JQ_BIN" -Rsa . 2>/dev/null || printf '""')"
  printf '{"ok":false,"error":"render_queue_failure","message":"queue updated but markdown render failed (exit %d)","detail":%s}\n' "$RENDER_STATUS" "$DETAIL"
  exit 1
fi

printf '%s\n' "$OUTPUT"

#!/usr/bin/env bash
# Refuse execute-next when the canonical queue lacks a current harness verdict.
#
# Usage:
#   queue_ready.sh [queue_path] [queue_validation_path]
#
# Exit codes:
#   0 = queue is ready for execution
#   1 = helper/runtime failure
#   2 = queue is not ready for execution

set -euo pipefail

QUEUE_PATH="${1:-slices/queue.json}"
QUEUE_VALIDATION_PATH="${2:-.mutagen/state/queue-validation.json}"

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

emit_failure() {
  local reason="$1"
  local message="$2"
  local exit_code="$3"
  local issues_json="${4:-[]}"
  local shadow_json="${5:-[]}"

  "$JQ_BIN" -n \
    --arg queue "$QUEUE_PATH" \
    --arg queue_validation "$QUEUE_VALIDATION_PATH" \
    --arg reason "$reason" \
    --arg message "$message" \
    --argjson issues "$issues_json" \
    --argjson shadow_artifacts "$shadow_json" \
    '{
      ok: false,
      queue: $queue,
      queue_validation: $queue_validation,
      reason: $reason,
      message: $message,
      issues: $issues,
      shadow_artifacts: $shadow_artifacts
    }'
  exit "$exit_code"
}

JQ_BIN="$(resolve_jq)" || {
  printf '{"ok":false,"reason":"tooling_failure","message":"jq not found on PATH"}\n'
  exit 1
}

if [[ "$QUEUE_PATH" != /* ]]; then
  QUEUE_PATH="$(pwd)/$QUEUE_PATH"
fi

if [[ "$QUEUE_VALIDATION_PATH" != /* ]]; then
  QUEUE_VALIDATION_PATH="$(pwd)/$QUEUE_VALIDATION_PATH"
fi

existing_shadow_files_json="$(
  {
    for path in "$(dirname "$QUEUE_PATH")/slicemap.md" "$(dirname "$QUEUE_PATH")/queue.md"; do
      if [[ -f "$path" ]]; then
        printf '%s\n' "$path"
      fi
    done
    true
  } \
    | "$JQ_BIN" -Rsc 'split("\n") | map(select(length > 0))'
)"

if [[ ! -f "$QUEUE_PATH" ]]; then
  if [[ -f "$QUEUE_VALIDATION_PATH" ]]; then
    emit_failure \
      "queue_validation_orphaned" \
      "Queue validation report is orphaned. The validator report exists but canonical queue JSON is missing. Re-run /mutagen:slice before /mutagen:execute-next." \
      2 \
      '[]' \
      "$existing_shadow_files_json"
  fi

  if [[ "$existing_shadow_files_json" != "[]" ]]; then
    emit_failure \
      "queue_json_missing" \
      "Canonical queue JSON is missing but markdown renderings exist. Re-run /mutagen:slice before /mutagen:execute-next." \
      2 \
      '[]' \
      "$existing_shadow_files_json"
  fi

  emit_failure \
    "queue_json_missing" \
    "Canonical queue JSON is missing. Re-run /mutagen:slice before /mutagen:execute-next." \
    2
fi

if [[ ! -f "$QUEUE_VALIDATION_PATH" ]]; then
  emit_failure \
    "queue_validation_missing" \
    "Queue validation report is missing. Re-run /mutagen:slice before /mutagen:execute-next." \
    2
fi

queue_mtime="$(stat -c %Y "$QUEUE_PATH" 2>/dev/null || echo 0)"
queue_validation_mtime="$(stat -c %Y "$QUEUE_VALIDATION_PATH" 2>/dev/null || echo 0)"

if [[ "$queue_mtime" -gt "$queue_validation_mtime" ]]; then
  emit_failure \
    "queue_validation_stale" \
    "Queue validation report is stale. slices/queue.json changed after validation. Re-run /mutagen:slice before /mutagen:execute-next." \
    2
fi

if ! "$JQ_BIN" empty "$QUEUE_VALIDATION_PATH" >/dev/null 2>&1; then
  emit_failure \
    "queue_validation_malformed" \
    "Queue validation report is malformed JSON. Re-run /mutagen:slice before /mutagen:execute-next." \
    2
fi

report_ok="$("$JQ_BIN" -r '
  if has("ok") then
    (.ok | if . == true then "true" elif . == false then "false" else "" end)
  else
    ""
  end
' "$QUEUE_VALIDATION_PATH" 2>/dev/null || true)"

if [[ "$report_ok" != "true" ]]; then
  issues_json="$("$JQ_BIN" -c '.issues // []' "$QUEUE_VALIDATION_PATH" 2>/dev/null || echo '[]')"
  validator_message="$("$JQ_BIN" -r '.message // empty' "$QUEUE_VALIDATION_PATH" 2>/dev/null || true)"
  message="Queue validation report says the queue is not executable. Fix Shredder output and re-run /mutagen:slice before /mutagen:execute-next."

  if [[ -n "$validator_message" ]]; then
    message="$message Validator said: $validator_message"
  fi

  emit_failure \
    "queue_validation_failed" \
    "$message" \
    2 \
    "$issues_json"
fi

"$JQ_BIN" -n \
  --arg queue "$QUEUE_PATH" \
  --arg queue_validation "$QUEUE_VALIDATION_PATH" \
  '{
    ok: true,
    queue: $queue,
    queue_validation: $queue_validation,
    message: "Queue validation is current and executable."
  }'

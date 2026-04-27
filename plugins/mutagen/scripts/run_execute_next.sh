#!/usr/bin/env bash

set -euo pipefail

WORKSPACE_ROOT="."
QUEUE_PATH="slices/queue.json"
QUEUE_VALIDATION_PATH=".mutagen/state/queue-validation.json"
WORKFLOW_CONFIG_PATH=".claude/workflow.json"
ACTIVE_STATE_PATH=".mutagen/state/active-slice.json"
AUTHOR_OUTPUT_DIR=".mutagen/state/author-output"
DISPATCH_ROOT=".mutagen/state/dispatch"
DISPATCH_LOG_PATH=".mutagen/state/dispatch-log.jsonl"
SUMMARY_ROOT="slices"
SLICEMAP_PATH="slices/slicemap.md"
LEGACY_PATH="slices/queue.md"
HOST_KIND="codex"
MAX_LOOPS=1000

usage() {
  cat <<'EOF' >&2
Usage:
  run_execute_next.sh [--workspace-root PATH] [--queue PATH]
    [--queue-validation PATH] [--workflow-config PATH] [--active-state PATH]
    [--author-output-dir PATH] [--dispatch-root PATH]
    [--dispatch-log PATH] [--summary-root PATH]
    [--slicemap PATH] [--legacy PATH]
    [--host HOST]
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

emit_error() {
  local error="$1"
  local message="$2"

  "$JQ_BIN" -n \
    --arg error "$error" \
    --arg message "$message" \
    '{
      ok: false,
      error: $error,
      message: $message
    }'
  exit 1
}

emit_terminal() {
  local status="$1"
  local terminal_json="$2"

  "$JQ_BIN" -n \
    --arg status "$status" \
    --argjson completed_slices "$COMPLETED_SLICES_JSON" \
    --argjson terminal "$terminal_json" \
    '{
      ok: true,
      status: $status,
      completed_count: ($completed_slices | length),
      completed_slices: $completed_slices,
      completion_markers: ($completed_slices | map(.completion_marker)),
      terminal: $terminal
    }'
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
    --author-output-dir)
      [[ $# -ge 2 ]] || usage
      AUTHOR_OUTPUT_DIR="$2"
      shift 2
      ;;
    --dispatch-root)
      [[ $# -ge 2 ]] || usage
      DISPATCH_ROOT="$2"
      shift 2
      ;;
    --dispatch-log)
      [[ $# -ge 2 ]] || usage
      DISPATCH_LOG_PATH="$2"
      shift 2
      ;;
    --summary-root)
      [[ $# -ge 2 ]] || usage
      SUMMARY_ROOT="$2"
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
    --host)
      [[ $# -ge 2 ]] || usage
      HOST_KIND="$2"
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

JQ_BIN="$(resolve_jq)" || {
  printf '{"ok":false,"error":"tooling_failure","message":"jq not found on PATH"}\n'
  exit 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

WORKSPACE_ROOT="$(absolute_path "$WORKSPACE_ROOT")"
cd "$WORKSPACE_ROOT" || emit_error "workspace_root_invalid" "failed to enter workspace root"

QUEUE_PATH="$(absolute_path "$QUEUE_PATH")"
QUEUE_VALIDATION_PATH="$(absolute_path "$QUEUE_VALIDATION_PATH")"
WORKFLOW_CONFIG_PATH="$(absolute_path "$WORKFLOW_CONFIG_PATH")"
ACTIVE_STATE_PATH="$(absolute_path "$ACTIVE_STATE_PATH")"
AUTHOR_OUTPUT_DIR="$(absolute_path "$AUTHOR_OUTPUT_DIR")"
DISPATCH_ROOT="$(absolute_path "$DISPATCH_ROOT")"
DISPATCH_LOG_PATH="$(absolute_path "$DISPATCH_LOG_PATH")"
SUMMARY_ROOT="$(absolute_path "$SUMMARY_ROOT")"
SLICEMAP_PATH="$(absolute_path "$SLICEMAP_PATH")"
LEGACY_PATH="$(absolute_path "$LEGACY_PATH")"

COMPLETED_SLICES_JSON='[]'
LOOP_GUARD=0
PAUSE_SENTINEL="$WORKSPACE_ROOT/.mutagen/state/pause.json"

while true; do
  LOOP_GUARD=$((LOOP_GUARD + 1))
  if [[ $LOOP_GUARD -gt $MAX_LOOPS ]]; then
    emit_error "loop_guard_exceeded" "execute-next runner exceeded its loop guard"
  fi

  # Stage-boundary pause: if an operator dropped a pause sentinel since the
  # previous iteration, stop here and surface the reason instead of claiming
  # the next slice. The harness does not pre-empt work already in flight; that
  # is by design (use OS signals if you need to kill an active dispatch).
  if [[ -f "$PAUSE_SENTINEL" ]]; then
    if [[ -s "$PAUSE_SENTINEL" ]] && "$JQ_BIN" empty "$PAUSE_SENTINEL" >/dev/null 2>&1; then
      pause_payload="$(cat "$PAUSE_SENTINEL")"
    else
      pause_payload='{}'
    fi
    "$JQ_BIN" -n \
      --argjson completed_slices "$COMPLETED_SLICES_JSON" \
      --argjson pause "$pause_payload" \
      --arg sentinel "$PAUSE_SENTINEL" \
      '{
        ok: true,
        status: "paused",
        completed_count: ($completed_slices | length),
        completed_slices: $completed_slices,
        completion_markers: ($completed_slices | map(.completion_marker)),
        pause: ($pause + {sentinel: $sentinel})
      }'
    exit 0
  fi

  set +e
  RUN_OUTPUT="$(
    bash "$SCRIPT_DIR/run_cohort_once.sh" \
      --workspace-root "$WORKSPACE_ROOT" \
      --queue "$QUEUE_PATH" \
      --queue-validation "$QUEUE_VALIDATION_PATH" \
      --workflow-config "$WORKFLOW_CONFIG_PATH" \
      --active-state "$ACTIVE_STATE_PATH" \
      --author-output-dir "$AUTHOR_OUTPUT_DIR" \
      --dispatch-root "$DISPATCH_ROOT" \
      --dispatch-log "$DISPATCH_LOG_PATH" \
      --summary-root "$SUMMARY_ROOT" \
      --slicemap "$SLICEMAP_PATH" \
      --legacy "$LEGACY_PATH" \
      --host "$HOST_KIND" 2>&1
  )"
  RUN_STATUS=$?
  set -e

  if [[ $RUN_STATUS -eq 2 ]]; then
    if printf '%s' "$RUN_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
      completed_from_terminal="$("$JQ_BIN" -c '.completed_slices // []' <<<"$RUN_OUTPUT")"
      COMPLETED_SLICES_JSON="$(printf '%s' "$COMPLETED_SLICES_JSON" | "$JQ_BIN" -c --argjson entries "$completed_from_terminal" '. + $entries')"
      "$JQ_BIN" -n \
        --argjson completed_slices "$COMPLETED_SLICES_JSON" \
        --argjson terminal "$RUN_OUTPUT" \
        '{
          ok: false,
          status: "queue_validation_failed",
          completed_count: ($completed_slices | length),
          completed_slices: $completed_slices,
          completion_markers: ($completed_slices | map(.completion_marker)),
          terminal: $terminal
        }'
      exit 2
    fi

    emit_error "run_slice_once_failed" "$RUN_OUTPUT"
  fi

  if [[ $RUN_STATUS -ne 0 ]]; then
    emit_error "run_cohort_once_failed" "$RUN_OUTPUT"
  fi

  if ! printf '%s' "$RUN_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
    emit_error "run_cohort_once_failed" "run_cohort_once.sh returned non-JSON output: $RUN_OUTPUT"
  fi

  RUN_RESULT_STATUS="$(printf '%s' "$RUN_OUTPUT" | "$JQ_BIN" -r '.status')"

  case "$RUN_RESULT_STATUS" in
    completed)
      completed_entries="$("$JQ_BIN" -c '.completed_slices // []' <<<"$RUN_OUTPUT")"
      COMPLETED_SLICES_JSON="$(printf '%s' "$COMPLETED_SLICES_JSON" | "$JQ_BIN" -c --argjson entries "$completed_entries" '. + $entries')"
      ;;
    queue_clear|stalled|escalated)
      completed_entries="$("$JQ_BIN" -c '.completed_slices // []' <<<"$RUN_OUTPUT")"
      COMPLETED_SLICES_JSON="$(printf '%s' "$COMPLETED_SLICES_JSON" | "$JQ_BIN" -c --argjson entries "$completed_entries" '. + $entries')"
      emit_terminal "$RUN_RESULT_STATUS" "$RUN_OUTPUT"
      exit 0
      ;;
    *)
      emit_error "run_cohort_once_failed" "run_cohort_once.sh returned unsupported status `$RUN_RESULT_STATUS`"
      ;;
  esac
done

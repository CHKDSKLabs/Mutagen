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
WORKTREE_ROOT=""

usage() {
  cat <<'EOF' >&2
Usage:
  run_cohort_once.sh [--workspace-root PATH] [--queue PATH]
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
  local extra_json="${3:-{}}"

  "$JQ_BIN" -n \
    --arg error "$error" \
    --arg message "$message" \
    --argjson extra "$extra_json" \
    '{
      ok: false,
      error: $error,
      message: $message
    } + $extra'
  exit 1
}

cleanup_worktree_root() {
  [[ -n "$WORKTREE_ROOT" ]] || return 0

  bash "$SCRIPT_DIR/cleanup_cohort_worktrees.sh" \
    --workspace-root "$WORKSPACE_ROOT" \
    --worktree-root "$WORKTREE_ROOT" >/dev/null 2>&1 || true
}

normalize_serial_result() {
  local prepare_output="$1"
  local run_output="$2"
  local run_status="$3"

  if [[ $run_status -eq 2 ]]; then
    "$JQ_BIN" -n \
      --argjson prepare_cohort "$prepare_output" \
      --argjson terminal "$run_output" \
      '{
        ok: false,
        status: "queue_validation_failed",
        mode: "serial_only",
        completed_count: 0,
        completed_slices: [],
        completion_markers: [],
        prepare_cohort: $prepare_cohort,
        terminal: $terminal
      }'
    exit 2
  fi

  if [[ $run_status -ne 0 ]]; then
    emit_error "run_slice_once_failed" "$run_output"
  fi

  if ! printf '%s' "$run_output" | "$JQ_BIN" empty >/dev/null 2>&1; then
    emit_error "run_slice_once_failed" "run_slice_once.sh returned non-JSON output: $run_output"
  fi

  local run_result_status
  run_result_status="$(printf '%s' "$run_output" | "$JQ_BIN" -r '.status')"

  case "$run_result_status" in
    completed)
      local completed_entry
      completed_entry="$(
        printf '%s' "$run_output" | "$JQ_BIN" -c '
          {
            slice_id,
            completion_marker: (.finalize.completion_marker // ""),
            review_skipped: (.review_skipped // false),
            summary_path: (.finalize.summary_path // null),
            worktree_path: null
          }
        '
      )"
      "$JQ_BIN" -n \
        --argjson prepare_cohort "$prepare_output" \
        --argjson terminal "$run_output" \
        --argjson completed_slices "[$completed_entry]" \
        '{
          ok: true,
          status: "completed",
          mode: "serial_only",
          completed_count: ($completed_slices | length),
          completed_slices: $completed_slices,
          completion_markers: ($completed_slices | map(.completion_marker)),
          prepare_cohort: $prepare_cohort,
          terminal: $terminal
        }'
      ;;
    queue_clear|stalled|escalated)
      "$JQ_BIN" -n \
        --arg status "$run_result_status" \
        --argjson prepare_cohort "$prepare_output" \
        --argjson terminal "$run_output" \
        '{
          ok: true,
          status: $status,
          mode: "serial_only",
          completed_count: 0,
          completed_slices: [],
          completion_markers: [],
          prepare_cohort: $prepare_cohort,
          terminal: $terminal
        }'
      ;;
    *)
      emit_error "run_slice_once_failed" "run_slice_once.sh returned unsupported status `$run_result_status`"
      ;;
  esac
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
trap cleanup_worktree_root EXIT

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

if [[ -f "$ACTIVE_STATE_PATH" ]]; then
  existing_slice_id="$("$JQ_BIN" -r '.slice_id // empty' "$ACTIVE_STATE_PATH" 2>/dev/null || true)"
  if [[ -n "$existing_slice_id" ]]; then
    emit_error \
      "active_slice_present" \
      "active-slice.json already exists for `$existing_slice_id`. Resolve or clear the current slice before starting another cohort."
  fi

  emit_error \
    "active_slice_present" \
    "active-slice.json already exists. Resolve or clear the current slice before starting another cohort."
fi

set +e
PREPARE_OUTPUT="$(
  bash "$SCRIPT_DIR/prepare_cohort.sh" \
    --workspace-root "$WORKSPACE_ROOT" \
    --queue "$QUEUE_PATH" \
    --queue-validation "$QUEUE_VALIDATION_PATH" \
    --workflow-config "$WORKFLOW_CONFIG_PATH" \
    --host "$HOST_KIND" 2>&1
)"
PREPARE_STATUS=$?
set -e

if [[ $PREPARE_STATUS -eq 2 ]]; then
  "$JQ_BIN" -n \
    --argjson terminal "$PREPARE_OUTPUT" \
    '{
      ok: false,
      status: "queue_validation_failed",
      mode: "prepare_cohort",
      completed_count: 0,
      completed_slices: [],
      completion_markers: [],
      terminal: $terminal
    }'
  exit 2
fi

if [[ $PREPARE_STATUS -ne 0 ]]; then
  emit_error "prepare_cohort_failed" "$PREPARE_OUTPUT"
fi

if ! printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "prepare_cohort_failed" "prepare_cohort.sh returned non-JSON output"
fi

PREPARE_RESULT_STATUS="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.status')"

case "$PREPARE_RESULT_STATUS" in
  queue_clear|stalled)
    "$JQ_BIN" -n \
      --arg status "$PREPARE_RESULT_STATUS" \
      --argjson prepare_cohort "$PREPARE_OUTPUT" \
      '{
        ok: true,
        status: $status,
        mode: "prepare_cohort",
        completed_count: 0,
        completed_slices: [],
        completion_markers: [],
        prepare_cohort: $prepare_cohort,
        terminal: $prepare_cohort
      }'
    exit 0
    ;;
  serial_only)
    set +e
    SERIAL_OUTPUT="$(
      bash "$SCRIPT_DIR/run_slice_once.sh" \
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
    SERIAL_STATUS=$?
    set -e
    normalize_serial_result "$PREPARE_OUTPUT" "$SERIAL_OUTPUT" "$SERIAL_STATUS"
    exit 0
    ;;
  ready)
    ;;
  *)
    emit_error "prepare_cohort_failed" "prepare-cohort returned unsupported status `$PREPARE_RESULT_STATUS`"
    ;;
esac

COHORT_COUNT="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.cohort | length')"
if [[ "$COHORT_COUNT" -le 1 ]]; then
  SELECTED_SLICE_ID="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.cohort[0].slice_id')"
  set +e
  SERIAL_OUTPUT="$(
    bash "$SCRIPT_DIR/run_slice_once.sh" \
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
      --host "$HOST_KIND" \
      --slice-id "$SELECTED_SLICE_ID" 2>&1
  )"
  SERIAL_STATUS=$?
  set -e
  normalize_serial_result "$PREPARE_OUTPUT" "$SERIAL_OUTPUT" "$SERIAL_STATUS"
  exit 0
fi

if ! git -C "$WORKSPACE_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  emit_error \
    "worktree_unavailable" \
    "bounded cohort execution requires a git worktree-capable repository"
fi

MATERIALIZE_ARGS=(
  bash "$SCRIPT_DIR/materialize_cohort_worktrees.sh"
  --workspace-root "$WORKSPACE_ROOT"
)

while IFS= read -r slice_id; do
  [[ -n "$slice_id" ]] || continue
  MATERIALIZE_ARGS+=(--slice-id "$slice_id")
done < <(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.cohort[]?.slice_id')

set +e
MATERIALIZE_OUTPUT="$("${MATERIALIZE_ARGS[@]}" 2>&1)"
MATERIALIZE_STATUS=$?
set -e

if [[ $MATERIALIZE_STATUS -ne 0 ]]; then
  emit_error "worktree_create_failed" "$MATERIALIZE_OUTPUT"
fi

if ! printf '%s' "$MATERIALIZE_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "worktree_create_failed" "materialize_cohort_worktrees.sh returned non-JSON output"
fi

WORKTREE_ROOT="$(printf '%s' "$MATERIALIZE_OUTPUT" | "$JQ_BIN" -r '.worktree_root')"

DISPATCH_ARGS=(
  bash "$SCRIPT_DIR/dispatch_cohort_members.sh"
  --workspace-root "$WORKSPACE_ROOT"
  --runner-script "$SCRIPT_DIR/run_slice_once.sh"
  --host "$HOST_KIND"
)

while IFS= read -r member_json; do
  DISPATCH_ARGS+=(--member-json "$member_json")
done < <(printf '%s' "$MATERIALIZE_OUTPUT" | "$JQ_BIN" -cr '.members[]')

set +e
DISPATCH_OUTPUT="$("${DISPATCH_ARGS[@]}" 2>&1)"
DISPATCH_STATUS=$?
set -e

if [[ $DISPATCH_STATUS -ne 0 ]]; then
  emit_error "cohort_member_failed" "$DISPATCH_OUTPUT"
fi

if ! printf '%s' "$DISPATCH_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "cohort_member_failed" "dispatch_cohort_members.sh returned non-JSON output: $DISPATCH_OUTPUT"
fi

APPLY_ARGS=(
  bash "$SCRIPT_DIR/apply_cohort_dispatch.sh"
  --workspace-root "$WORKSPACE_ROOT"
  --queue "$QUEUE_PATH"
  --dispatch-log "$DISPATCH_LOG_PATH"
  --slicemap "$SLICEMAP_PATH"
  --legacy "$LEGACY_PATH"
)

while IFS= read -r member_json; do
  APPLY_ARGS+=(--member-json "$member_json")
done < <(printf '%s' "$DISPATCH_OUTPUT" | "$JQ_BIN" -cr '.members[]')

set +e
APPLY_OUTPUT="$("${APPLY_ARGS[@]}" 2>&1)"
APPLY_STATUS=$?
set -e

if [[ $APPLY_STATUS -ne 0 ]]; then
  emit_error "apply_cohort_dispatch_failed" "$APPLY_OUTPUT"
fi

if ! printf '%s' "$APPLY_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "apply_cohort_dispatch_failed" "apply_cohort_dispatch.sh returned non-JSON output: $APPLY_OUTPUT"
fi

APPLY_STATUS_KIND="$(printf '%s' "$APPLY_OUTPUT" | "$JQ_BIN" -r '.status')"

case "$APPLY_STATUS_KIND" in
  completed)
    "$JQ_BIN" -n \
      --argjson prepare_cohort "$PREPARE_OUTPUT" \
      --argjson applied "$APPLY_OUTPUT" \
      '{
        ok: true,
        status: "completed",
        mode: "bounded_cohort",
        cohort_size: $applied.completed_count,
        completed_count: $applied.completed_count,
        completed_slices: $applied.completed_slices,
        completion_markers: $applied.completion_markers,
        prepare_cohort: $prepare_cohort
      }'
    exit 0
    ;;
  escalated)
    "$JQ_BIN" -n \
      --argjson applied "$APPLY_OUTPUT" \
      '{
        ok: true,
        status: "escalated",
        slice_id: $applied.slice_id,
        worktree_path: $applied.worktree_path,
        completed_count: $applied.completed_count,
        completed_slices: $applied.completed_slices,
        completion_markers: $applied.completion_markers,
        terminal: $applied.terminal
      }
      + (if ($applied.stage // null) != null then { stage: $applied.stage } else {} end)
      + (if ($applied.stop_condition // null) != null then { stop_condition: $applied.stop_condition } else {} end)
      + (if ($applied.conflicting_slice_id // null) != null then { conflicting_slice_id: $applied.conflicting_slice_id } else {} end)
      + (if ($applied.conflicting_path // null) != null then { conflicting_path: $applied.conflicting_path } else {} end)'
    ;;
  failed)
    "$JQ_BIN" -n \
      --argjson applied "$APPLY_OUTPUT" \
      '{
        ok: false,
        error: "cohort_member_failed",
        slice_id: $applied.slice_id,
        worktree_path: $applied.worktree_path,
        completed_count: $applied.completed_count,
        completed_slices: $applied.completed_slices,
        completion_markers: $applied.completion_markers,
        message: $applied.message
      }'
    exit 1
    ;;
  *)
    emit_error "apply_cohort_dispatch_failed" "apply_cohort_dispatch.sh returned unsupported status '$APPLY_STATUS_KIND'"
    ;;
esac

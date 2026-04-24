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

safe_file_name() {
  local value="$1"
  local safe=""
  local i ch

  for ((i = 0; i < ${#value}; i++)); do
    ch="${value:i:1}"
    case "$ch" in
      [[:alnum:]._-])
        safe+="$ch"
        ;;
      *)
        safe+="_"
        ;;
    esac
  done

  printf '%s\n' "$safe"
}

append_completed_entry() {
  local entry_json="$1"
  COMPLETED_SLICES_JSON="$(
    printf '%s' "$COMPLETED_SLICES_JSON" | "$JQ_BIN" -c --argjson entry "$entry_json" '. + [$entry]'
  )"
}

apply_imports_from_reconcile() {
  local reconcile_output="$1"
  local worktree="$2"

  while IFS=$'\t' read -r import_status import_path; do
    [[ -z "$import_path" ]] && continue
    import_entry "$import_status" "$import_path" "$worktree"
  done < <(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.imports[]? | [.status, .path] | @tsv')
}

record_merged_paths_from_reconcile() {
  local reconcile_output="$1"
  local slice_id="$2"

  while IFS= read -r import_path; do
    [[ -z "$import_path" ]] && continue
    MERGED_PATH_OWNERS["$import_path"]="$slice_id"
  done < <(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.imports[]?.path')
}

sync_queue_from_reconcile() {
  local reconcile_output="$1"
  local slice_id status attempts micro_used karai bishop tiger micro_correction completed_at escalation_reason

  slice_id="$(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.slice_id')"
  status="$(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.queue_sync.status')"
  attempts="$(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.queue_sync.attempts // 0')"
  micro_used="$(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.queue_sync.micro_corrections_used // 0')"
  karai="$(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.queue_sync.karai_structural // empty')"
  bishop="$(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.queue_sync.bishop // empty')"
  tiger="$(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.queue_sync.tiger_claw // empty')"
  micro_correction="$(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.queue_sync.micro_correction // empty')"
  completed_at="$(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.queue_sync.completed_at // empty')"
  escalation_reason="$(printf '%s' "$reconcile_output" | "$JQ_BIN" -r '.queue_sync.escalation_reason // empty')"

  local args=(
    bash "$SCRIPT_DIR/update_queue_slice.sh"
    --queue "$QUEUE_PATH"
    --slicemap "$SLICEMAP_PATH"
    --legacy "$LEGACY_PATH"
    --slice-id "$slice_id"
    --status "$status"
    --attempts "$attempts"
    --micro-corrections-used "$micro_used"
  )

  [[ -n "$karai" ]] && args+=(--karai-structural "$karai")
  [[ -n "$bishop" ]] && args+=(--bishop "$bishop")
  [[ -n "$tiger" ]] && args+=(--tiger-claw "$tiger")
  [[ -n "$micro_correction" ]] && args+=(--micro-correction "$micro_correction")
  [[ -n "$completed_at" ]] && args+=(--completed-at "$completed_at")
  [[ -n "$escalation_reason" ]] && args+=(--escalation-reason "$escalation_reason")

  local update_output
  set +e
  update_output="$("${args[@]}" 2>&1)"
  local update_status=$?
  set -e

  if [[ $update_status -ne 0 ]]; then
    emit_error "queue_sync_failed" "$update_output" "{\"slice_id\":\"$slice_id\"}"
  fi
}

path_matches_glob() {
  local path="$1"
  local glob="$2"

  [[ "$path" == $glob ]]
}

path_allows_shared_import() {
  local path="$1"

  [[ "$path" == ".mutagen/state/tiger-claw-latest.md" ]]
}

path_allowed_for_completed() {
  local path="$1"
  local slice_json="$2"
  local slice_id safe_id

  slice_id="$(printf '%s' "$slice_json" | "$JQ_BIN" -r '.slice_id')"
  safe_id="$(safe_file_name "$slice_id")"

  while IFS= read -r glob; do
    [[ -z "$glob" ]] && continue
    if path_matches_glob "$path" "$glob"; then
      return 0
    fi
  done < <(printf '%s' "$slice_json" | "$JQ_BIN" -r '.selection_scope[]?')

  case "$path" in
    "reviews/$slice_id/"*|\
    "slices/$slice_id/"*|\
    ".mutagen/state/author-output/$safe_id.md"|\
    ".mutagen/state/review-output/$safe_id.md"|\
    ".mutagen/state/evidence/$safe_id.md"|\
    ".mutagen/state/tiger-claw-latest.md"|\
    ".mutagen/state/dispatch/$slice_id/"*|\
    "tests/qa/"*)
      return 0
      ;;
  esac

  return 1
}

path_allowed_for_diagnostics() {
  local path="$1"
  local slice_id="$2"
  local safe_id

  safe_id="$(safe_file_name "$slice_id")"

  case "$path" in
    "reviews/$slice_id/"*|\
    ".mutagen/state/author-output/$safe_id.md"|\
    ".mutagen/state/review-output/$safe_id.md"|\
    ".mutagen/state/evidence/$safe_id.md"|\
    ".mutagen/state/tiger-claw-latest.md"|\
    ".mutagen/state/dispatch/$slice_id/"*)
      return 0
      ;;
  esac

  return 1
}

path_differs_from_main() {
  local path="$1"
  local status="$2"
  local worktree_path="$3"
  local main_path="$4"

  if [[ "$status" == "D" ]]; then
    [[ -e "$main_path" ]]
    return
  fi

  if [[ ! -e "$worktree_path" ]]; then
    return 1
  fi

  if [[ ! -e "$main_path" ]]; then
    return 0
  fi

  if cmp -s "$worktree_path" "$main_path"; then
    return 1
  fi

  return 0
}

collect_delta_entries() {
  local worktree="$1"

  while IFS= read -r -d '' status && IFS= read -r -d '' path; do
    printf '%s\t%s\n' "$status" "$path"
  done < <(git -C "$worktree" diff --name-status -z --no-renames --diff-filter=ADM --relative HEAD --)

  while IFS= read -r -d '' path; do
    printf 'A\t%s\n' "$path"
  done < <(git -C "$worktree" ls-files --others --exclude-standard -z)
}

build_import_entries() {
  local worktree="$1"
  local slice_json="$2"
  local mode="$3"
  local slice_id

  slice_id="$(printf '%s' "$slice_json" | "$JQ_BIN" -r '.slice_id')"

  while IFS=$'\t' read -r status path; do
    [[ -z "$path" ]] && continue

    if [[ "$mode" == "completed" ]]; then
      path_allowed_for_completed "$path" "$slice_json" || continue
    else
      path_allowed_for_diagnostics "$path" "$slice_id" || continue
    fi

    if ! path_differs_from_main "$path" "$status" "$worktree/$path" "$WORKSPACE_ROOT/$path"; then
      continue
    fi

    printf '%s\t%s\n' "$status" "$path"
  done < <(collect_delta_entries "$worktree")
}

import_entry() {
  local status="$1"
  local path="$2"
  local worktree="$3"

  if [[ "$status" == "D" ]]; then
    rm -f "$WORKSPACE_ROOT/$path"
    return 0
  fi

  mkdir -p "$(dirname "$WORKSPACE_ROOT/$path")"
  cp -f "$worktree/$path" "$WORKSPACE_ROOT/$path"
}

append_dispatch_log_entry() {
  local worktree="$1"
  local slice_id="$2"
  local worktree_log="$worktree/.mutagen/state/dispatch-log.jsonl"

  [[ -f "$worktree_log" ]] || return 0

  local log_entry
  log_entry="$(awk -v slice_id="\"slice_id\":\"$slice_id\"" 'index($0, slice_id) { line = $0 } END { print line }' "$worktree_log")"
  [[ -n "$log_entry" ]] || return 0

  mkdir -p "$(dirname "$DISPATCH_LOG_PATH")"
  if [[ -f "$DISPATCH_LOG_PATH" ]] && rg -F "\"slice_id\":\"$slice_id\"" "$DISPATCH_LOG_PATH" >/dev/null 2>&1; then
    return 0
  fi

  printf '%s\n' "$log_entry" >>"$DISPATCH_LOG_PATH"
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

COMPLETED_SLICES_JSON='[]'

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

declare -a PIDS=()

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

while IFS= read -r member_json; do
  slice_id="$(printf '%s' "$member_json" | "$JQ_BIN" -r '.slice_id')"
  worktree_path="$(printf '%s' "$member_json" | "$JQ_BIN" -r '.worktree_path')"
  result_path="$(printf '%s' "$member_json" | "$JQ_BIN" -r '.result_path')"
  status_path="$(printf '%s' "$member_json" | "$JQ_BIN" -r '.status_path')"

  (
    set +e
    bash "$SCRIPT_DIR/run_slice_once.sh" \
      --workspace-root "$worktree_path" \
      --queue "$worktree_path/slices/queue.json" \
      --queue-validation "$worktree_path/.mutagen/state/queue-validation.json" \
      --workflow-config "$worktree_path/.claude/workflow.json" \
      --active-state "$worktree_path/.mutagen/state/active-slice.json" \
      --author-output-dir "$worktree_path/.mutagen/state/author-output" \
      --dispatch-root "$worktree_path/.mutagen/state/dispatch" \
      --dispatch-log "$worktree_path/.mutagen/state/dispatch-log.jsonl" \
      --summary-root "$worktree_path/slices" \
      --slicemap "$worktree_path/slices/slicemap.md" \
      --legacy "$worktree_path/slices/queue.md" \
      --host "$HOST_KIND" \
      --slice-id "$slice_id" >"$result_path" 2>&1
    printf '%s\n' "$?" >"$status_path"
  ) &

  PIDS+=("$!")
done < <(printf '%s' "$MATERIALIZE_OUTPUT" | "$JQ_BIN" -cr '.members[]')

for pid in "${PIDS[@]}"; do
  wait "$pid" || true
done

declare -A MERGED_PATH_OWNERS=()

while IFS= read -r member_json; do
  slice_id="$(printf '%s' "$member_json" | "$JQ_BIN" -r '.slice_id')"
  worktree_path="$(printf '%s' "$member_json" | "$JQ_BIN" -r '.worktree_path')"
  result_path="$(printf '%s' "$member_json" | "$JQ_BIN" -r '.result_path')"
  status_path="$(printf '%s' "$member_json" | "$JQ_BIN" -r '.status_path')"
  worktree_path_json="$("$JQ_BIN" -Rn --arg value "$worktree_path" '$value')"

  set +e
  COLLECT_OUTPUT="$(
    bash "$SCRIPT_DIR/collect_cohort_member_result.sh" \
      --workspace-root "$WORKSPACE_ROOT" \
      --worktree-root "$worktree_path" \
      --slice-id "$slice_id" \
      --result-path "$result_path" \
      --status-path "$status_path" 2>&1
  )"
  COLLECT_STATUS=$?
  set -e

  if [[ $COLLECT_STATUS -ne 0 ]]; then
    emit_error \
      "cohort_member_failed" \
      "$COLLECT_OUTPUT" \
      "{\"slice_id\":\"$slice_id\",\"worktree_path\":$worktree_path_json}"
  fi

  if ! printf '%s' "$COLLECT_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
    emit_error \
      "cohort_member_failed" \
      "collect_cohort_member_result.sh returned non-JSON output: $COLLECT_OUTPUT" \
      "{\"slice_id\":\"$slice_id\",\"worktree_path\":$worktree_path_json}"
  fi

  COLLECT_STATUS_KIND="$(printf '%s' "$COLLECT_OUTPUT" | "$JQ_BIN" -r '.status')"
  if [[ "$COLLECT_STATUS_KIND" == "failed" ]]; then
    RUN_OUTPUT="$(printf '%s' "$COLLECT_OUTPUT" | "$JQ_BIN" -r '.message')"
    "$JQ_BIN" -n \
      --arg slice_id "$slice_id" \
      --arg worktree_path "$worktree_path" \
      --arg message "$RUN_OUTPUT" \
      --argjson completed_slices "$COMPLETED_SLICES_JSON" \
      '{
        ok: false,
        error: "cohort_member_failed",
        slice_id: $slice_id,
        worktree_path: $worktree_path,
        completed_count: ($completed_slices | length),
        completed_slices: $completed_slices,
        completion_markers: ($completed_slices | map(.completion_marker)),
        message: $message
      }'
    exit 1
  fi

  RUN_OUTPUT="$(printf '%s' "$COLLECT_OUTPUT" | "$JQ_BIN" -c '.run_output')"
  MEMBER_STATUS="$(printf '%s' "$COLLECT_OUTPUT" | "$JQ_BIN" -r '.member_status')"

  MERGED_OWNER_ARGS=()
  for merged_path in "${!MERGED_PATH_OWNERS[@]}"; do
    MERGED_OWNER_ARGS+=(--merged-path-owner "$merged_path=${MERGED_PATH_OWNERS[$merged_path]}")
  done

  set +e
  RECONCILE_OUTPUT="$(
    bash "$SCRIPT_DIR/reconcile_cohort_member.sh" \
      --workspace-root "$WORKSPACE_ROOT" \
      --worktree-root "$worktree_path" \
      --slice-id "$slice_id" \
      --run-output "$result_path" \
      "${MERGED_OWNER_ARGS[@]}" 2>&1
  )"
  RECONCILE_STATUS_CODE=$?
  set -e

  if [[ $RECONCILE_STATUS_CODE -ne 0 ]]; then
    emit_error \
      "reconcile_cohort_member_failed" \
      "$RECONCILE_OUTPUT" \
      "{\"slice_id\":\"$slice_id\",\"worktree_path\":$worktree_path_json}"
  fi

  if ! printf '%s' "$RECONCILE_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
    emit_error \
      "reconcile_cohort_member_failed" \
      "reconcile_cohort_member.sh returned non-JSON output: $RECONCILE_OUTPUT" \
      "{\"slice_id\":\"$slice_id\",\"worktree_path\":$worktree_path_json}"
  fi

  RECONCILE_STATUS="$(printf '%s' "$RECONCILE_OUTPUT" | "$JQ_BIN" -r '.status')"

  case "$RECONCILE_STATUS" in
    completed)
      apply_imports_from_reconcile "$RECONCILE_OUTPUT" "$worktree_path"
      set +e
      STATE_UPDATE_OUTPUT="$(
        bash "$SCRIPT_DIR/apply_state_update.sh" \
          --workspace-root "$WORKSPACE_ROOT" \
          --queue "$QUEUE_PATH" \
          --slice-id "$slice_id" \
          --author-output "$(printf '%s' "$RECONCILE_OUTPUT" | "$JQ_BIN" -r '.author_output_path')" 2>&1
      )"
      STATE_UPDATE_STATUS=$?
      set -e

      if [[ $STATE_UPDATE_STATUS -ne 0 ]]; then
        emit_error \
          "state_update_apply_failed" \
          "$STATE_UPDATE_OUTPUT" \
          "{\"slice_id\":\"$slice_id\",\"worktree_path\":$worktree_path_json}"
      fi

      sync_queue_from_reconcile "$RECONCILE_OUTPUT"
      append_dispatch_log_entry "$worktree_path" "$slice_id"
      record_merged_paths_from_reconcile "$RECONCILE_OUTPUT" "$slice_id"
      completed_entry="$(printf '%s' "$RECONCILE_OUTPUT" | "$JQ_BIN" -c '.completed_entry')"
      append_completed_entry "$completed_entry"
      ;;
    escalated)
      apply_imports_from_reconcile "$RECONCILE_OUTPUT" "$worktree_path"
      sync_queue_from_reconcile "$RECONCILE_OUTPUT"
      "$JQ_BIN" -n \
        --arg slice_id "$slice_id" \
        --arg worktree_path "$worktree_path" \
        --argjson completed_slices "$COMPLETED_SLICES_JSON" \
        --argjson terminal "$RUN_OUTPUT" \
        '{
          ok: true,
          status: "escalated",
          slice_id: $slice_id,
          worktree_path: $worktree_path,
          completed_count: ($completed_slices | length),
          completed_slices: $completed_slices,
          completion_markers: ($completed_slices | map(.completion_marker)),
              terminal: $terminal
            }'
      exit 0
      ;;
    merge_conflict)
      apply_imports_from_reconcile "$RECONCILE_OUTPUT" "$worktree_path"
      sync_queue_from_reconcile "$RECONCILE_OUTPUT"

      "$JQ_BIN" -n \
        --arg slice_id "$slice_id" \
        --arg conflicting_slice_id "$(printf '%s' "$RECONCILE_OUTPUT" | "$JQ_BIN" -r '.conflicting_slice_id')" \
        --arg conflicting_path "$(printf '%s' "$RECONCILE_OUTPUT" | "$JQ_BIN" -r '.conflicting_path')" \
        --arg worktree_path "$worktree_path" \
        --argjson completed_slices "$COMPLETED_SLICES_JSON" \
        --argjson terminal "$RUN_OUTPUT" \
        '{
          ok: true,
          status: "escalated",
          stage: "cohort_merge",
          stop_condition: "merge_conflict",
          slice_id: $slice_id,
          conflicting_slice_id: $conflicting_slice_id,
          conflicting_path: $conflicting_path,
          worktree_path: $worktree_path,
          completed_count: ($completed_slices | length),
          completed_slices: $completed_slices,
          completion_markers: ($completed_slices | map(.completion_marker)),
          terminal: $terminal
        }'
      exit 0
      ;;
    *)
      emit_error \
        "cohort_member_failed" \
        "reconcile_cohort_member.sh returned unsupported status `$RECONCILE_STATUS`" \
        "{\"slice_id\":\"$slice_id\",\"worktree_path\":$worktree_path_json}"
      ;;
  esac
done < <(printf '%s' "$MATERIALIZE_OUTPUT" | "$JQ_BIN" -cr '.members[]')

"$JQ_BIN" -n \
  --argjson prepare_cohort "$PREPARE_OUTPUT" \
  --argjson completed_slices "$COMPLETED_SLICES_JSON" \
  '{
    ok: true,
    status: "completed",
    mode: "bounded_cohort",
    cohort_size: ($completed_slices | length),
    completed_count: ($completed_slices | length),
    completed_slices: $completed_slices,
    completion_markers: ($completed_slices | map(.completion_marker)),
    prepare_cohort: $prepare_cohort
  }'

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
CURRENT_SLICE_ID=""
SELECTED_SLICE_ID=""

usage() {
  cat <<'EOF' >&2
Usage:
  run_slice_once.sh [--workspace-root PATH] [--queue PATH]
    [--queue-validation PATH] [--workflow-config PATH] [--active-state PATH]
    [--author-output-dir PATH] [--dispatch-root PATH]
    [--dispatch-log PATH] [--summary-root PATH]
    [--slicemap PATH] [--legacy PATH]
    [--host HOST] [--slice-id ID]
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
    --arg slice_id "$CURRENT_SLICE_ID" \
    '{
      ok: false,
      error: $error,
      message: $message
    } + (if $slice_id != "" then {slice_id: $slice_id} else {} end)'
  exit 1
}

run_json_step() {
  local label="$1"
  shift

  local output=""
  local status=0

  set +e
  output="$("$@" 2>&1)"
  status=$?
  set -e

  if [[ $status -ne 0 ]]; then
    emit_error "${label}_failed" "$output"
  fi

  if ! printf '%s' "$output" | "$JQ_BIN" empty >/dev/null 2>&1; then
    emit_error "${label}_failed" "${label} returned non-JSON output: $output"
  fi

  printf '%s\n' "$output"
}

join_findings() {
  local payload="$1"
  local joined=""

  joined="$(
    printf '%s' "$payload" | "$JQ_BIN" -r '
      [.findings[]?.detail // empty]
      | map(select(length > 0))
      | join(" | ")
    '
  )"

  if [[ -z "$joined" ]]; then
    joined="Karai structural fail"
  fi

  printf '%s\n' "$joined"
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
    --slice-id)
      [[ $# -ge 2 ]] || usage
      SELECTED_SLICE_ID="$2"
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

mkdir -p \
  "$WORKSPACE_ROOT/.mutagen/state" \
  "$WORKSPACE_ROOT/.mutagen/state/evidence" \
  "$WORKSPACE_ROOT/reviews" \
  "$WORKSPACE_ROOT/slices"

if [[ -f "$ACTIVE_STATE_PATH" ]]; then
  existing_slice_id="$("$JQ_BIN" -r '.slice_id // empty' "$ACTIVE_STATE_PATH" 2>/dev/null || true)"
  if [[ -n "$existing_slice_id" ]]; then
    CURRENT_SLICE_ID="$existing_slice_id"
    emit_error \
      "active_slice_present" \
      "active-slice.json already exists for `$existing_slice_id`. Resolve or clear the current slice before starting another."
  fi

  emit_error \
    "active_slice_present" \
    "active-slice.json already exists. Resolve or clear the current slice before starting another."
fi

set +e
if [[ -n "$SELECTED_SLICE_ID" ]]; then
  CURRENT_SLICE_ID="$SELECTED_SLICE_ID"
  PREPARE_OUTPUT="$(
    bash "$SCRIPT_DIR/prepare_selected_slice.sh" \
      --workspace-root "$WORKSPACE_ROOT" \
      --queue "$QUEUE_PATH" \
      --workflow-config "$WORKFLOW_CONFIG_PATH" \
      --active-state "$ACTIVE_STATE_PATH" \
      --slice-id "$SELECTED_SLICE_ID" \
      --host "$HOST_KIND" 2>&1
  )"
else
  PREPARE_OUTPUT="$(
    bash "$SCRIPT_DIR/prepare_next.sh" \
      --workspace-root "$WORKSPACE_ROOT" \
      --queue "$QUEUE_PATH" \
      --queue-validation "$QUEUE_VALIDATION_PATH" \
      --workflow-config "$WORKFLOW_CONFIG_PATH" \
      --active-state "$ACTIVE_STATE_PATH" \
      --host "$HOST_KIND" 2>&1
  )"
fi
PREPARE_STATUS=$?
set -e

if [[ $PREPARE_STATUS -eq 2 ]]; then
  printf '%s\n' "$PREPARE_OUTPUT"
  exit 2
fi

if [[ $PREPARE_STATUS -ne 0 ]]; then
  emit_error "prepare_slice_failed" "$PREPARE_OUTPUT"
fi

if ! printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "prepare_slice_failed" "slice preparation returned non-JSON output"
fi

PREPARE_RESULT_STATUS="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.status')"

case "$PREPARE_RESULT_STATUS" in
  queue_clear|stalled)
    "$JQ_BIN" -n \
      --arg status "$PREPARE_RESULT_STATUS" \
      --argjson prepare_next "$PREPARE_OUTPUT" \
      '{
        ok: true,
        status: $status,
        prepare_next: $prepare_next
      }'
    exit 0
    ;;
  ready)
    CURRENT_SLICE_ID="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.slice_id')"
    ;;
  *)
    emit_error "prepare_slice_failed" "slice preparation returned unsupported status `$PREPARE_RESULT_STATUS`"
    ;;
esac

MAX_LOOPS="$("$JQ_BIN" -r '
  ((.max_retries // 2) + (.max_micro_corrections // 1) + 4)
' "$ACTIVE_STATE_PATH")"

if [[ -z "$MAX_LOOPS" || "$MAX_LOOPS" == "null" ]]; then
  MAX_LOOPS=7
fi

DISPATCH_KIND=""
QA_REPORT_PATH=""
LATEST_QA_REPORT_PATH=""
ACTIVE_AGENT_OVERRIDE=""
REVIEW_SKIPPED=false
LAST_REVIEW_DISPATCH='null'
LAST_REVIEW_DECISION='null'
LAST_SKIP_UPDATE='null'
LAST_STRUCTURAL='null'
LAST_STRUCTURAL_UPDATE='null'
LOOP_GUARD=0

while true; do
  LOOP_GUARD=$((LOOP_GUARD + 1))
  if [[ $LOOP_GUARD -gt $MAX_LOOPS ]]; then
    emit_error "loop_guard_exceeded" "slice runner exceeded its retry guard while processing `$CURRENT_SLICE_ID`"
  fi

  AUTHOR_TRANSITION_ARGS=(
    bash "$SCRIPT_DIR/transition_active_slice.sh"
    --queue "$QUEUE_PATH"
    --active-state "$ACTIVE_STATE_PATH"
    --slicemap "$SLICEMAP_PATH"
    --legacy "$LEGACY_PATH"
    --slice-id "$CURRENT_SLICE_ID"
    --stage author
  )

  if [[ -n "$ACTIVE_AGENT_OVERRIDE" ]]; then
    AUTHOR_TRANSITION_ARGS+=(--active-agent "$ACTIVE_AGENT_OVERRIDE")
  fi

  if [[ "$DISPATCH_KIND" == "micro_correction" ]]; then
    AUTHOR_TRANSITION_ARGS+=(--bump-micro-corrections)
  else
    AUTHOR_TRANSITION_ARGS+=(--bump-attempts)
  fi

  AUTHOR_TRANSITION_OUTPUT="$(run_json_step "author_transition" "${AUTHOR_TRANSITION_ARGS[@]}")"

  AUTHOR_DISPATCH_ARGS=(
    bash "$SCRIPT_DIR/dispatch_stage.sh"
    --workspace-root "$WORKSPACE_ROOT"
    --queue "$QUEUE_PATH"
    --active-state "$ACTIVE_STATE_PATH"
    --author-output-dir "$AUTHOR_OUTPUT_DIR"
    --dispatch-root "$DISPATCH_ROOT"
    --slicemap "$SLICEMAP_PATH"
    --legacy "$LEGACY_PATH"
    --slice-id "$CURRENT_SLICE_ID"
  )

  if [[ -n "$DISPATCH_KIND" ]]; then
    AUTHOR_DISPATCH_ARGS+=(--dispatch-kind "$DISPATCH_KIND")
  fi

  if [[ -n "$QA_REPORT_PATH" ]]; then
    AUTHOR_DISPATCH_ARGS+=(--qa-report "$QA_REPORT_PATH")
  fi

  if [[ -n "$LATEST_QA_REPORT_PATH" ]]; then
    AUTHOR_DISPATCH_ARGS+=(--latest-qa-report "$LATEST_QA_REPORT_PATH")
  fi

  AUTHOR_DISPATCH_OUTPUT="$(run_json_step "author_dispatch" "${AUTHOR_DISPATCH_ARGS[@]}")"

  STRUCTURAL_TRANSITION_OUTPUT="$(
    run_json_step \
      "structural_transition" \
      bash "$SCRIPT_DIR/transition_active_slice.sh" \
      --queue "$QUEUE_PATH" \
      --active-state "$ACTIVE_STATE_PATH" \
      --slicemap "$SLICEMAP_PATH" \
      --legacy "$LEGACY_PATH" \
      --slice-id "$CURRENT_SLICE_ID" \
      --stage structural-check
  )"

  LAST_STRUCTURAL="$(run_json_step "structural_check" bash "$SCRIPT_DIR/karai_structural_check.sh" "$CURRENT_SLICE_ID")"
  STRUCTURAL_VERDICT="$(printf '%s' "$LAST_STRUCTURAL" | "$JQ_BIN" -r '.verdict')"

  if [[ "$STRUCTURAL_VERDICT" == "fail" ]]; then
    STRUCTURAL_REASON="$(join_findings "$LAST_STRUCTURAL")"
    LAST_STRUCTURAL_UPDATE="$(
      run_json_step \
        "structural_queue_update" \
        bash "$SCRIPT_DIR/update_queue_slice.sh" \
        --queue "$QUEUE_PATH" \
        --slicemap "$SLICEMAP_PATH" \
        --legacy "$LEGACY_PATH" \
        --slice-id "$CURRENT_SLICE_ID" \
        --status escalated \
        --karai-structural fail \
        --escalation-reason "$STRUCTURAL_REASON"
    )"

    STOP_CONDITION="$(printf '%s' "$LAST_STRUCTURAL" | "$JQ_BIN" -r '.stop_condition // "structural_failure"')"

    "$JQ_BIN" -n \
      --arg slice_id "$CURRENT_SLICE_ID" \
      --arg stop_condition "$STOP_CONDITION" \
      --argjson prepare_next "$PREPARE_OUTPUT" \
      --argjson author_transition "$AUTHOR_TRANSITION_OUTPUT" \
      --argjson author_dispatch "$AUTHOR_DISPATCH_OUTPUT" \
      --argjson structural_transition "$STRUCTURAL_TRANSITION_OUTPUT" \
      --argjson structural "$LAST_STRUCTURAL" \
      --argjson structural_queue_update "$LAST_STRUCTURAL_UPDATE" \
      '{
        ok: true,
        status: "escalated",
        stage: "structural_check",
        slice_id: $slice_id,
        stop_condition: $stop_condition,
        prepare_next: $prepare_next,
        author_transition: $author_transition,
        author_dispatch: $author_dispatch,
        structural_transition: $structural_transition,
        structural: $structural,
        structural_queue_update: $structural_queue_update
      }'
    exit 0
  fi

  LAST_STRUCTURAL_UPDATE="$(
    run_json_step \
      "structural_queue_update" \
      bash "$SCRIPT_DIR/update_queue_slice.sh" \
      --queue "$QUEUE_PATH" \
      --slicemap "$SLICEMAP_PATH" \
      --legacy "$LEGACY_PATH" \
      --slice-id "$CURRENT_SLICE_ID" \
      --karai-structural pass
  )"

  PIPELINE_MODE="$("$JQ_BIN" -r '.pipeline_mode' "$ACTIVE_STATE_PATH")"
  REVIEW_REQUIRED="$("$JQ_BIN" -r '.review_required' "$ACTIVE_STATE_PATH")"

  if [[ "$PIPELINE_MODE" == "lightweight" && "$REVIEW_REQUIRED" == "false" ]]; then
    REVIEW_SKIPPED=true
    LAST_SKIP_UPDATE="$(
      run_json_step \
        "review_skip_update" \
        bash "$SCRIPT_DIR/update_queue_slice.sh" \
        --queue "$QUEUE_PATH" \
        --slicemap "$SLICEMAP_PATH" \
        --legacy "$LEGACY_PATH" \
        --slice-id "$CURRENT_SLICE_ID" \
        --bishop skip \
        --tiger-claw skip
    )"
    break
  fi

  REVIEW_TRANSITION_OUTPUT="$(
    run_json_step \
      "review_transition" \
      bash "$SCRIPT_DIR/transition_active_slice.sh" \
      --queue "$QUEUE_PATH" \
      --active-state "$ACTIVE_STATE_PATH" \
      --slicemap "$SLICEMAP_PATH" \
      --legacy "$LEGACY_PATH" \
      --slice-id "$CURRENT_SLICE_ID" \
      --stage review
  )"

  LAST_REVIEW_DISPATCH="$(
    run_json_step \
      "review_dispatch" \
      bash "$SCRIPT_DIR/dispatch_stage.sh" \
      --workspace-root "$WORKSPACE_ROOT" \
      --queue "$QUEUE_PATH" \
      --active-state "$ACTIVE_STATE_PATH" \
      --author-output-dir "$AUTHOR_OUTPUT_DIR" \
      --dispatch-root "$DISPATCH_ROOT" \
      --slicemap "$SLICEMAP_PATH" \
      --legacy "$LEGACY_PATH" \
      --slice-id "$CURRENT_SLICE_ID"
  )"

  LAST_REVIEW_DECISION="$(
    run_json_step \
      "review_decision" \
      bash "$SCRIPT_DIR/review_decision.sh" \
      --workspace-root "$WORKSPACE_ROOT" \
      --queue "$QUEUE_PATH" \
      --active-state "$ACTIVE_STATE_PATH" \
      --slicemap "$SLICEMAP_PATH" \
      --legacy "$LEGACY_PATH" \
      --slice-id "$CURRENT_SLICE_ID"
  )"

  REVIEW_ACTION="$(printf '%s' "$LAST_REVIEW_DECISION" | "$JQ_BIN" -r '.action')"

  case "$REVIEW_ACTION" in
    continue)
      break
      ;;
    micro_correction)
      DISPATCH_KIND="micro_correction"
      QA_REPORT_PATH="$(printf '%s' "$LAST_REVIEW_DECISION" | "$JQ_BIN" -r '.qa_report_path')"
      LATEST_QA_REPORT_PATH="$(printf '%s' "$LAST_REVIEW_DECISION" | "$JQ_BIN" -r '.latest_qa_report_path')"
      ACTIVE_AGENT_OVERRIDE="$(printf '%s' "$LAST_REVIEW_DECISION" | "$JQ_BIN" -r '.active_agent')"
      continue
      ;;
    retry)
      DISPATCH_KIND="retry"
      QA_REPORT_PATH="$(printf '%s' "$LAST_REVIEW_DECISION" | "$JQ_BIN" -r '.qa_report_path')"
      LATEST_QA_REPORT_PATH="$(printf '%s' "$LAST_REVIEW_DECISION" | "$JQ_BIN" -r '.latest_qa_report_path')"
      ACTIVE_AGENT_OVERRIDE=""
      continue
      ;;
    escalated)
      STOP_CONDITION="$(printf '%s' "$LAST_REVIEW_DECISION" | "$JQ_BIN" -r '.stop_condition // "retry_budget_exhausted"')"
      "$JQ_BIN" -n \
        --arg slice_id "$CURRENT_SLICE_ID" \
        --arg stop_condition "$STOP_CONDITION" \
        --argjson prepare_next "$PREPARE_OUTPUT" \
        --argjson author_transition "$AUTHOR_TRANSITION_OUTPUT" \
        --argjson author_dispatch "$AUTHOR_DISPATCH_OUTPUT" \
        --argjson structural_transition "$STRUCTURAL_TRANSITION_OUTPUT" \
        --argjson structural "$LAST_STRUCTURAL" \
        --argjson structural_queue_update "$LAST_STRUCTURAL_UPDATE" \
        --argjson review_transition "$REVIEW_TRANSITION_OUTPUT" \
        --argjson review_dispatch "$LAST_REVIEW_DISPATCH" \
        --argjson review_decision "$LAST_REVIEW_DECISION" \
        '{
          ok: true,
          status: "escalated",
          stage: "review",
          slice_id: $slice_id,
          stop_condition: $stop_condition,
          prepare_next: $prepare_next,
          author_transition: $author_transition,
          author_dispatch: $author_dispatch,
          structural_transition: $structural_transition,
          structural: $structural,
          structural_queue_update: $structural_queue_update,
          review_transition: $review_transition,
          review_dispatch: $review_dispatch,
          review_decision: $review_decision
        }'
      exit 0
      ;;
    *)
      emit_error "review_decision_failed" "review-decision returned unsupported action `$REVIEW_ACTION`"
      ;;
  esac
done

STATE_RECORD_TRANSITION_OUTPUT="$(
  run_json_step \
    "state_record_transition" \
    bash "$SCRIPT_DIR/transition_active_slice.sh" \
    --queue "$QUEUE_PATH" \
    --active-state "$ACTIVE_STATE_PATH" \
    --slicemap "$SLICEMAP_PATH" \
    --legacy "$LEGACY_PATH" \
    --slice-id "$CURRENT_SLICE_ID" \
    --stage state-record
)"

FINALIZE_OUTPUT="$(
  run_json_step \
    "finalize_slice" \
    bash "$SCRIPT_DIR/finalize_slice.sh" \
    --workspace-root "$WORKSPACE_ROOT" \
    --queue "$QUEUE_PATH" \
    --active-state "$ACTIVE_STATE_PATH" \
    --dispatch-log "$DISPATCH_LOG_PATH" \
    --summary-root "$SUMMARY_ROOT" \
    --slicemap "$SLICEMAP_PATH" \
    --legacy "$LEGACY_PATH" \
    --slice-id "$CURRENT_SLICE_ID"
)"

"$JQ_BIN" -n \
  --arg slice_id "$CURRENT_SLICE_ID" \
  --argjson prepare_next "$PREPARE_OUTPUT" \
  --argjson structural "$LAST_STRUCTURAL" \
  --argjson finalize "$FINALIZE_OUTPUT" \
  --argjson state_record_transition "$STATE_RECORD_TRANSITION_OUTPUT" \
  --argjson review_dispatch "$LAST_REVIEW_DISPATCH" \
  --argjson review_decision "$LAST_REVIEW_DECISION" \
  --argjson review_skip_update "$LAST_SKIP_UPDATE" \
  --arg review_skipped "$REVIEW_SKIPPED" \
  '{
    ok: true,
    status: "completed",
    slice_id: $slice_id,
    review_skipped: ($review_skipped == "true"),
    prepare_next: $prepare_next,
    structural: $structural,
    review_dispatch: $review_dispatch,
    review_decision: $review_decision,
    review_skip_update: $review_skip_update,
    state_record_transition: $state_record_transition,
    finalize: $finalize
  }'

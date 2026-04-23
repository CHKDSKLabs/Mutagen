#!/usr/bin/env bash

set -euo pipefail

WORKSPACE_ROOT="."
QUEUE_PATH="slices/queue.json"
ACTIVE_STATE_PATH=".mutagen/state/active-slice.json"
AUTHOR_OUTPUT_DIR=".mutagen/state/author-output"
DISPATCH_ROOT=".mutagen/state/dispatch"
SLICEMAP_PATH="slices/slicemap.md"
LEGACY_PATH="slices/queue.md"
SLICE_ID=""
QA_REPORT_PATH=""
LATEST_QA_REPORT_PATH=""
DISPATCH_KIND=""

usage() {
  cat <<'EOF' >&2
Usage:
  dispatch_stage.sh --slice-id ID
                    [--workspace-root PATH]
                    [--queue PATH]
                    [--active-state PATH]
                    [--author-output-dir PATH]
                    [--dispatch-root PATH]
                    [--slicemap PATH]
                    [--legacy PATH]
                    [--qa-report PATH]
                    [--latest-qa-report PATH]
                    [--dispatch-kind initial|retry|micro_correction]
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
    --slice-id)
      [[ $# -ge 2 ]] || usage
      SLICE_ID="$2"
      shift 2
      ;;
    --qa-report)
      [[ $# -ge 2 ]] || usage
      QA_REPORT_PATH="$2"
      shift 2
      ;;
    --latest-qa-report)
      [[ $# -ge 2 ]] || usage
      LATEST_QA_REPORT_PATH="$2"
      shift 2
      ;;
    --dispatch-kind)
      [[ $# -ge 2 ]] || usage
      DISPATCH_KIND="$2"
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

[[ -n "$SLICE_ID" ]] || usage

JQ_BIN="$(resolve_jq)" || {
  printf '{"ok":false,"reason":"tooling_failure","message":"jq not found on PATH"}\n'
  exit 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/harness/Cargo.toml"
MUTAGEN_ROOT="${MUTAGEN_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"
export MUTAGEN_ROOT

if [[ ! -f "$MANIFEST_PATH" ]]; then
  "$JQ_BIN" -n \
    --arg manifest "$MANIFEST_PATH" \
    '{
      ok: false,
      reason: "dispatch_stage_unavailable",
      message: ("mutagen harness manifest not found at " + $manifest)
    }'
  exit 1
fi

CARGO_BIN="$(resolve_cargo)" || {
  "$JQ_BIN" -n \
    --arg slice_id "$SLICE_ID" \
    '{
      ok: false,
      reason: "dispatch_stage_unavailable",
      slice_id: $slice_id,
      message: "cargo not found on PATH"
    }'
  exit 1
}

WORKSPACE_ROOT="$(absolute_path "$WORKSPACE_ROOT")"
QUEUE_PATH="$(absolute_path "$QUEUE_PATH")"
ACTIVE_STATE_PATH="$(absolute_path "$ACTIVE_STATE_PATH")"
AUTHOR_OUTPUT_DIR="$(absolute_path "$AUTHOR_OUTPUT_DIR")"
DISPATCH_ROOT="$(absolute_path "$DISPATCH_ROOT")"
SLICEMAP_PATH="$(absolute_path "$SLICEMAP_PATH")"
LEGACY_PATH="$(absolute_path "$LEGACY_PATH")"

prepare_args=(
  "$CARGO_BIN" run
  --quiet
  --manifest-path "$MANIFEST_PATH"
  --
  prepare-dispatch
  --workspace-root "$WORKSPACE_ROOT"
  --queue "$QUEUE_PATH"
  --active-state "$ACTIVE_STATE_PATH"
  --author-output-dir "$AUTHOR_OUTPUT_DIR"
  --dispatch-root "$DISPATCH_ROOT"
  --slice-id "$SLICE_ID"
)

if [[ -n "$QA_REPORT_PATH" ]]; then
  prepare_args+=(--qa-report "$(absolute_path "$QA_REPORT_PATH")")
fi

if [[ -n "$LATEST_QA_REPORT_PATH" ]]; then
  prepare_args+=(--latest-qa-report "$(absolute_path "$LATEST_QA_REPORT_PATH")")
fi

if [[ -n "$DISPATCH_KIND" ]]; then
  prepare_args+=(--dispatch-kind "$DISPATCH_KIND")
fi

set +e
PREPARE_OUTPUT="$("${prepare_args[@]}" 2>&1)"
PREPARE_STATUS=$?
set -e

if [[ $PREPARE_STATUS -ne 0 ]]; then
  printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -Rs \
    --arg slice_id "$SLICE_ID" \
    '{
      ok: false,
      reason: "prepare_dispatch_runtime_failure",
      slice_id: $slice_id,
      message: .
    }'
  exit 1
fi

if ! printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -Rs \
    --arg slice_id "$SLICE_ID" \
    '{
      ok: false,
      reason: "prepare_dispatch_runtime_failure",
      slice_id: $slice_id,
      message: ("prepare-dispatch returned non-JSON output: " + .)
    }'
  exit 1
fi

AGENT_NAME="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.agent')"
PROMPT_PATH="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.prompt_path')"
STDOUT_CAPTURE_PATH="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.stdout_capture_path')"
STAGE_NAME="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.stage')"
PREPARED_QA_REPORT_PATH="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.qa_report_path // empty')"
PREPARED_LATEST_QA_REPORT_PATH="$(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.latest_qa_report_path // empty')"

mkdir -p "$(dirname "$STDOUT_CAPTURE_PATH")"

set +e
bash "$MUTAGEN_ROOT/bin/agent.sh" "$AGENT_NAME" "$(cat "$PROMPT_PATH")" >"$STDOUT_CAPTURE_PATH" 2>&1
AGENT_STATUS=$?
set -e

if [[ $AGENT_STATUS -ne 0 ]]; then
  "$JQ_BIN" -n \
    --argjson prepared "$PREPARE_OUTPUT" \
    --arg stage "$STAGE_NAME" \
    --arg agent "$AGENT_NAME" \
    --arg stdout_capture_path "$STDOUT_CAPTURE_PATH" \
    '{
      ok: false,
      reason: "agent_dispatch_failed",
      stage: $stage,
      agent: $agent,
      stdout_capture_path: $stdout_capture_path,
      prepared: $prepared
    }'
  exit 1
fi

mapfile -t REQUIRED_ARTIFACTS < <(printf '%s' "$PREPARE_OUTPUT" | "$JQ_BIN" -r '.required_written_artifacts[]?')

missing_artifacts=()
for artifact in "${REQUIRED_ARTIFACTS[@]}"; do
  [[ -f "$artifact" ]] || missing_artifacts+=("$artifact")
done

if [[ ${#missing_artifacts[@]} -gt 0 ]]; then
  printf '%s\n' "${missing_artifacts[@]}" | "$JQ_BIN" -Rsc \
    --argjson prepared "$PREPARE_OUTPUT" \
    --arg stage "$STAGE_NAME" \
    --arg agent "$AGENT_NAME" \
    --arg stdout_capture_path "$STDOUT_CAPTURE_PATH" \
    'split("\n") | map(select(length > 0)) as $missing | {
      ok: false,
      reason: "required_artifacts_missing",
      stage: $stage,
      agent: $agent,
      stdout_capture_path: $stdout_capture_path,
      missing_artifacts: $missing,
      prepared: $prepared
    }'
  exit 1
fi

REVIEW_RECORD_OUTPUT='null'

if [[ "$STAGE_NAME" == "review" ]]; then
  if [[ -z "$PREPARED_QA_REPORT_PATH" || -z "$PREPARED_LATEST_QA_REPORT_PATH" ]]; then
    "$JQ_BIN" -n \
      --argjson prepared "$PREPARE_OUTPUT" \
      --arg stage "$STAGE_NAME" \
      --arg agent "$AGENT_NAME" \
      --arg stdout_capture_path "$STDOUT_CAPTURE_PATH" \
      '{
        ok: false,
        reason: "review_verdict_record_failed",
        stage: $stage,
        agent: $agent,
        stdout_capture_path: $stdout_capture_path,
        message: "prepare-dispatch omitted review report paths",
        prepared: $prepared
      }'
    exit 1
  fi

  set +e
  REVIEW_RECORD_OUTPUT="$(
    "$SCRIPT_DIR/record_review_verdict.sh" \
      --workspace-root "$WORKSPACE_ROOT" \
      --queue "$QUEUE_PATH" \
      --active-state "$ACTIVE_STATE_PATH" \
      --qa-report "$PREPARED_QA_REPORT_PATH" \
      --latest-qa-report "$PREPARED_LATEST_QA_REPORT_PATH" \
      --slicemap "$SLICEMAP_PATH" \
      --legacy "$LEGACY_PATH" \
      --slice-id "$SLICE_ID" 2>&1
  )"
  REVIEW_RECORD_STATUS=$?
  set -e

  if [[ $REVIEW_RECORD_STATUS -ne 0 ]]; then
    printf '%s' "$REVIEW_RECORD_OUTPUT" | "$JQ_BIN" -Rs \
      --argjson prepared "$PREPARE_OUTPUT" \
      --arg stage "$STAGE_NAME" \
      --arg agent "$AGENT_NAME" \
      --arg stdout_capture_path "$STDOUT_CAPTURE_PATH" \
      '{
        ok: false,
        reason: "review_verdict_record_failed",
        stage: $stage,
        agent: $agent,
        stdout_capture_path: $stdout_capture_path,
        message: .,
        prepared: $prepared
      }'
    exit 1
  fi

  if ! printf '%s' "$REVIEW_RECORD_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
    printf '%s' "$REVIEW_RECORD_OUTPUT" | "$JQ_BIN" -Rs \
      --argjson prepared "$PREPARE_OUTPUT" \
      --arg stage "$STAGE_NAME" \
      --arg agent "$AGENT_NAME" \
      --arg stdout_capture_path "$STDOUT_CAPTURE_PATH" \
      '{
        ok: false,
        reason: "review_verdict_record_failed",
        stage: $stage,
        agent: $agent,
        stdout_capture_path: $stdout_capture_path,
        message: ("record_review_verdict.sh returned non-JSON output: " + .),
        prepared: $prepared
      }'
    exit 1
  fi
fi

"$JQ_BIN" -n \
  --argjson prepared "$PREPARE_OUTPUT" \
  --argjson review_record "$REVIEW_RECORD_OUTPUT" \
  --arg stage "$STAGE_NAME" \
  --arg agent "$AGENT_NAME" \
  --arg stdout_capture_path "$STDOUT_CAPTURE_PATH" \
  '{
    ok: true,
    stage: $stage,
    agent: $agent,
    stdout_capture_path: $stdout_capture_path,
    review_record: $review_record,
    prepared: $prepared
  }'

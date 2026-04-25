#!/usr/bin/env bash

set -euo pipefail

WORKSPACE_ROOT="."
QUEUE_PATH="slices/queue.json"
ACTIVE_STATE_PATH=".mutagen/state/active-slice.json"
QA_REPORT_PATH=""
LATEST_QA_REPORT_PATH=""
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
  record_review_verdict.sh [--workspace-root PATH] [--queue PATH] [--active-state PATH]
    [--qa-report PATH] [--latest-qa-report PATH]
    [--slicemap PATH] [--legacy PATH]
    --slice-id ID
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace-root)
      [[ $# -ge 2 ]] || usage
      WORKSPACE_ROOT="$2"
      HARNESS_ARGS+=("$1" "$2")
      shift 2
      ;;
    --queue)
      [[ $# -ge 2 ]] || usage
      QUEUE_PATH="$2"
      HARNESS_ARGS+=("$1" "$2")
      shift 2
      ;;
    --active-state)
      [[ $# -ge 2 ]] || usage
      ACTIVE_STATE_PATH="$2"
      HARNESS_ARGS+=("$1" "$2")
      shift 2
      ;;
    --qa-report)
      [[ $# -ge 2 ]] || usage
      QA_REPORT_PATH="$2"
      HARNESS_ARGS+=("$1" "$2")
      shift 2
      ;;
    --latest-qa-report)
      [[ $# -ge 2 ]] || usage
      LATEST_QA_REPORT_PATH="$2"
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
    --slice-id)
      [[ $# -ge 2 ]] || usage
      HARNESS_ARGS+=("$1" "$2")
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

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JQ_BIN="$(resolve_jq)" || emit_error "record_review_verdict_unavailable" "jq not found on PATH"

set +e
OUTPUT="$(
  bash "$SCRIPT_DIR/harness_runtime.sh" record-review-verdict "${HARNESS_ARGS[@]}" 2>&1
)"
STATUS=$?
set -e

if [[ $STATUS -ne 0 ]]; then
  emit_error "record_review_verdict_runtime_failure" "mutagen harness record-review-verdict runtime failed"
fi

if ! printf '%s' "$OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "record_review_verdict_runtime_failure" "mutagen harness record-review-verdict returned non-JSON output"
fi

set +e
"$SCRIPT_DIR/render_queue.sh" "$QUEUE_PATH" "$SLICEMAP_PATH" "$LEGACY_PATH" >/dev/null 2>&1
RENDER_STATUS=$?
set -e

if [[ $RENDER_STATUS -ne 0 ]]; then
  emit_error "render_queue_failure" "review verdict recorded but markdown render failed"
fi

printf '%s\n' "$OUTPUT"

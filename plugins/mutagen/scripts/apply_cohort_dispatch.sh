#!/usr/bin/env bash

set -euo pipefail

QUEUE_PATH="slices/queue.json"
DISPATCH_LOG_PATH=".mutagen/state/dispatch-log.jsonl"
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
  apply_cohort_dispatch.sh [--workspace-root PATH] [--queue PATH]
    [--dispatch-log PATH] [--slicemap PATH] [--legacy PATH]
    --member-json JSON
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace-root|--member-json)
      [[ $# -ge 2 ]] || usage
      HARNESS_ARGS+=("$1" "$2")
      shift 2
      ;;
    --queue)
      [[ $# -ge 2 ]] || usage
      QUEUE_PATH="$2"
      HARNESS_ARGS+=("$1" "$2")
      shift 2
      ;;
    --dispatch-log)
      [[ $# -ge 2 ]] || usage
      DISPATCH_LOG_PATH="$2"
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
    --help|-h)
      usage
      ;;
    *)
      usage
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JQ_BIN="$(resolve_jq)" || emit_error "apply_cohort_dispatch_unavailable" "jq not found on PATH"

set +e
OUTPUT="$(
  bash "$SCRIPT_DIR/harness_runtime.sh" apply-cohort-dispatch "${HARNESS_ARGS[@]}" 2>&1
)"
STATUS=$?
set -e

if [[ $STATUS -ne 0 ]]; then
  emit_error "apply_cohort_dispatch_runtime_failure" "mutagen harness apply-cohort-dispatch runtime failed"
fi

if ! printf '%s' "$OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "apply_cohort_dispatch_runtime_failure" "mutagen harness apply-cohort-dispatch returned non-JSON output"
fi

set +e
"$SCRIPT_DIR/render_queue.sh" "$QUEUE_PATH" "$SLICEMAP_PATH" "$LEGACY_PATH" >/dev/null 2>&1
RENDER_STATUS=$?
set -e

if [[ $RENDER_STATUS -ne 0 ]]; then
  emit_error "render_queue_failure" "cohort dispatch applied but markdown render failed"
fi

printf '%s\n' "$OUTPUT"

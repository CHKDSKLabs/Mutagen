#!/usr/bin/env bash

set -euo pipefail

WORKSPACE_ROOT="."
QUEUE_PATH="slices/queue.json"
ACTIVE_STATE_PATH=".mutagen/state/active-slice.json"
DISPATCH_LOG_PATH=".mutagen/state/dispatch-log.jsonl"
SUMMARY_ROOT="slices"
SLICEMAP_PATH="slices/slicemap.md"
LEGACY_PATH="slices/queue.md"
COMPLETED_AT=""
HARNESS_ARGS=()

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
  finalize_slice.sh [--workspace-root PATH] [--queue PATH] [--active-state PATH]
    [--dispatch-log PATH] [--summary-root PATH] [--slicemap PATH] [--legacy PATH]
    --slice-id ID
    [--completed-at ISO-8601]
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
    --dispatch-log)
      [[ $# -ge 2 ]] || usage
      DISPATCH_LOG_PATH="$2"
      HARNESS_ARGS+=("$1" "$2")
      shift 2
      ;;
    --summary-root)
      [[ $# -ge 2 ]] || usage
      SUMMARY_ROOT="$2"
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
    --slice-id|--completed-at)
      [[ $# -ge 2 ]] || usage
      if [[ "$1" == "--completed-at" ]]; then
        COMPLETED_AT="$2"
      fi
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
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/harness/Cargo.toml"

if [[ ! -f "$MANIFEST_PATH" ]]; then
  emit_error "finalize_slice_unavailable" "mutagen harness manifest not found"
fi

CARGO_BIN="$(resolve_cargo)" || emit_error "finalize_slice_unavailable" "cargo not found on PATH"
JQ_BIN="$(resolve_jq)" || emit_error "finalize_slice_unavailable" "jq not found on PATH"

if [[ -z "$COMPLETED_AT" ]]; then
  COMPLETED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")" || emit_error "finalize_slice_unavailable" "failed to compute completed-at timestamp"
  HARNESS_ARGS+=("--completed-at" "$COMPLETED_AT")
fi

set +e
OUTPUT="$(
  "$CARGO_BIN" run --quiet --manifest-path "$MANIFEST_PATH" -- finalize-slice "${HARNESS_ARGS[@]}" 2>&1
)"
STATUS=$?
set -e

if [[ $STATUS -ne 0 ]]; then
  emit_error "finalize_slice_runtime_failure" "mutagen harness finalize-slice runtime failed"
fi

if ! printf '%s' "$OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "finalize_slice_runtime_failure" "mutagen harness finalize-slice returned non-JSON output"
fi

printf '%s\n' "$OUTPUT" | "$SCRIPT_DIR/emit_notifications.sh" >/dev/null 2>&1 || true

set +e
"$SCRIPT_DIR/render_queue.sh" "$QUEUE_PATH" "$SLICEMAP_PATH" "$LEGACY_PATH" >/dev/null 2>&1
RENDER_STATUS=$?
set -e

if [[ $RENDER_STATUS -ne 0 ]]; then
  emit_error "render_queue_failure" "slice finalized but markdown render failed"
fi

printf '%s\n' "$OUTPUT"

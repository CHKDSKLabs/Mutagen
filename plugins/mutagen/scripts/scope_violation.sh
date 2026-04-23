#!/usr/bin/env bash

set -euo pipefail

WORKSPACE_ROOT="."
QUEUE_PATH="slices/queue.json"
ACTIVE_STATE_PATH=".mutagen/state/active-slice.json"
VIOLATION_PATH=".mutagen/state/scope-violation.json"
SLICEMAP_PATH="slices/slicemap.md"
LEGACY_PATH="slices/queue.md"

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
  scope_violation.sh [--workspace-root PATH] [--queue PATH] [--active-state PATH]
    [--violation-report PATH] [--slicemap PATH] [--legacy PATH]
EOF
  exit 1
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
    --violation-report)
      [[ $# -ge 2 ]] || usage
      VIOLATION_PATH="$2"
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
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/harness/Cargo.toml"

if [[ ! -f "$MANIFEST_PATH" ]]; then
  emit_error "scope_violation_unavailable" "mutagen harness manifest not found"
fi

CARGO_BIN="$(resolve_cargo)" || emit_error "scope_violation_unavailable" "cargo not found on PATH"
JQ_BIN="$(resolve_jq)" || emit_error "scope_violation_unavailable" "jq not found on PATH"

set +e
OUTPUT="$(
  "$CARGO_BIN" run --quiet --manifest-path "$MANIFEST_PATH" -- scope-violation \
    --workspace-root "$WORKSPACE_ROOT" \
    --queue "$QUEUE_PATH" \
    --active-state "$ACTIVE_STATE_PATH" \
    --violation-report "$VIOLATION_PATH" 2>&1
)"
STATUS=$?
set -e

if [[ $STATUS -ne 0 ]]; then
  emit_error "scope_violation_runtime_failure" "mutagen harness scope-violation runtime failed"
fi

if ! printf '%s' "$OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "scope_violation_runtime_failure" "mutagen harness scope-violation returned non-JSON output"
fi

printf '%s\n' "$OUTPUT" | "$SCRIPT_DIR/emit_notifications.sh" >/dev/null 2>&1 || true

if [[ "$(printf '%s' "$OUTPUT" | "$JQ_BIN" -r '.queue_updated // false')" == "true" ]]; then
  set +e
  "$SCRIPT_DIR/render_queue.sh" "$QUEUE_PATH" "$SLICEMAP_PATH" "$LEGACY_PATH" >/dev/null 2>&1
  RENDER_STATUS=$?
  set -e

  if [[ $RENDER_STATUS -ne 0 ]]; then
    emit_error "render_queue_failure" "scope violation normalized but markdown render failed"
  fi
fi

printf '%s\n' "$OUTPUT"

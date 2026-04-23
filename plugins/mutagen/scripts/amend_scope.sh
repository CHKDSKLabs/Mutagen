#!/usr/bin/env bash

set -euo pipefail

WORKSPACE_ROOT="."
QUEUE_PATH="slices/queue.json"
ACTIVE_STATE_PATH=".mutagen/state/active-slice.json"
AMENDMENTS_LOG_PATH=".mutagen/state/amendments.jsonl"
MUTATION_KIND=""
REASON=""
REQUESTED_GLOBS=()

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
  amend_scope.sh --requested-glob GLOB [--requested-glob GLOB ...]
    --mutation-kind create|modify|delete --reason TEXT
    [--workspace-root PATH] [--queue PATH] [--active-state PATH]
    [--amendments-log PATH]
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
    --amendments-log)
      [[ $# -ge 2 ]] || usage
      AMENDMENTS_LOG_PATH="$2"
      shift 2
      ;;
    --requested-glob)
      [[ $# -ge 2 ]] || usage
      REQUESTED_GLOBS+=("$2")
      shift 2
      ;;
    --mutation-kind)
      [[ $# -ge 2 ]] || usage
      MUTATION_KIND="$2"
      shift 2
      ;;
    --reason)
      [[ $# -ge 2 ]] || usage
      REASON="$2"
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

[[ ${#REQUESTED_GLOBS[@]} -gt 0 ]] || emit_error "amend_scope_invalid_args" "at least one --requested-glob is required"
[[ -n "$MUTATION_KIND" ]] || emit_error "amend_scope_invalid_args" "--mutation-kind is required"
[[ -n "$REASON" ]] || emit_error "amend_scope_invalid_args" "--reason is required"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/harness/Cargo.toml"

if [[ ! -f "$MANIFEST_PATH" ]]; then
  emit_error "amend_scope_unavailable" "mutagen harness manifest not found"
fi

CARGO_BIN="$(resolve_cargo)" || emit_error "amend_scope_unavailable" "cargo not found on PATH"
JQ_BIN="$(resolve_jq)" || emit_error "amend_scope_unavailable" "jq not found on PATH"

HARNESS_ARGS=(
  --workspace-root "$WORKSPACE_ROOT"
  --queue "$QUEUE_PATH"
  --active-state "$ACTIVE_STATE_PATH"
  --amendments-log "$AMENDMENTS_LOG_PATH"
  --mutation-kind "$MUTATION_KIND"
  --reason "$REASON"
)

for requested_glob in "${REQUESTED_GLOBS[@]}"; do
  HARNESS_ARGS+=(--requested-glob "$requested_glob")
done

set +e
OUTPUT="$(
  "$CARGO_BIN" run --quiet --manifest-path "$MANIFEST_PATH" -- amend-scope "${HARNESS_ARGS[@]}" 2>&1
)"
STATUS=$?
set -e

if [[ $STATUS -ne 0 ]]; then
  emit_error "amend_scope_runtime_failure" "mutagen harness amend-scope runtime failed"
fi

if ! printf '%s' "$OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "amend_scope_runtime_failure" "mutagen harness amend-scope returned non-JSON output"
fi

printf '%s\n' "$OUTPUT"

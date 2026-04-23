#!/usr/bin/env bash

set -euo pipefail

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
  apply_state_update.sh [--workspace-root PATH] [--queue PATH]
                        [--author-output PATH]
                        --slice-id ID
EOF
  exit 1
}

HARNESS_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace-root|--queue|--slice-id|--author-output)
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
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/harness/Cargo.toml"

[[ -f "$MANIFEST_PATH" ]] || emit_error "apply_state_update_unavailable" "mutagen harness manifest not found"

CARGO_BIN="$(resolve_cargo)" || emit_error "apply_state_update_unavailable" "cargo not found on PATH"
JQ_BIN="$(resolve_jq)" || emit_error "apply_state_update_unavailable" "jq not found on PATH"

set +e
OUTPUT="$(
  "$CARGO_BIN" run --quiet --manifest-path "$MANIFEST_PATH" -- apply-state-update "${HARNESS_ARGS[@]}" 2>&1
)"
STATUS=$?
set -e

if [[ $STATUS -ne 0 ]]; then
  emit_error "apply_state_update_runtime_failure" "mutagen harness apply-state-update runtime failed"
fi

if ! printf '%s' "$OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  emit_error "apply_state_update_runtime_failure" "mutagen harness apply-state-update returned non-JSON output"
fi

printf '%s\n' "$OUTPUT"

#!/usr/bin/env bash

set -u

emit_fail() {
  local check="$1"
  local detail="$2"
  printf '{"verdict":"fail","findings":[{"check":"%s","severity":"fail","detail":"%s"}],"loc":{}}\n' "$check" "$detail"
  exit 0
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

slice_id="${1:-}"
if [ -z "$slice_id" ]; then
  emit_fail "args" "missing slice_id"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/harness/Cargo.toml"
WORKSPACE_ROOT="$(pwd)"

if [ ! -f "$MANIFEST_PATH" ]; then
  emit_fail "tooling" "mutagen harness manifest not found"
fi

CARGO_BIN="$(resolve_cargo)" || emit_fail "tooling" "cargo not found"

set +e
REPORT="$(
  "$CARGO_BIN" run --quiet --manifest-path "$MANIFEST_PATH" -- structural-check "$slice_id" \
    --workspace-root "$WORKSPACE_ROOT" \
    --queue "slices/queue.json" \
    --author-output-dir ".mutagen/state/author-output" \
    --loc-script "plugins/mutagen/scripts/slice_loc.sh" 2>&1
)"
STATUS=$?
set -e

if [ $STATUS -ne 0 ]; then
  emit_fail "tooling" "mutagen harness structural-check runtime failed"
fi

case "$REPORT" in
  *'"verdict"'*)
    printf '%s\n' "$REPORT" | "$SCRIPT_DIR/emit_notifications.sh" >/dev/null 2>&1 || true
    printf '%s\n' "$REPORT"
    ;;
  *)
    emit_fail "tooling" "mutagen harness structural-check returned non-JSON output"
    ;;
esac

#!/usr/bin/env bash

set -u

emit_fail() {
  local check="$1"
  local detail="$2"
  printf '{"verdict":"fail","findings":[{"check":"%s","severity":"fail","detail":"%s"}],"loc":{}}\n' "$check" "$detail"
  exit 0
}


slice_id="${1:-}"
if [ -z "$slice_id" ]; then
  emit_fail "args" "missing slice_id"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(pwd)"

set +e
REPORT="$(
  bash "$SCRIPT_DIR/harness_runtime.sh" structural-check "$slice_id" \
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

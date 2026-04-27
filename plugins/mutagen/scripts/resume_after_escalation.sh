#!/usr/bin/env bash
# Resume a slice that was paused by a manual repair. Replaces the four-step
# manual sequence (structural-check, update_queue_slice, transition_active_slice,
# dispatch-stage) with one call so an operator does not have to remember the
# exact order.
#
# Typical use: an author dispatch wrote a partial artifact, the operator hand-
# repaired it on disk, and the queue should now resume from the structural
# check stage as if the repair was the original output.
#
# Usage:
#   resume_after_escalation.sh --slice-id ID
#                              [--stage structural-check|review|state-record]
#                              [--workspace-root PATH]
#                              [--queue PATH]
#                              [--active-state PATH]
#                              [--author-output-dir PATH]
#                              [--dispatch-root PATH]
#                              [--slicemap PATH]
#                              [--legacy PATH]
#                              [--host HOST]
#                              [--reset-status]
#
# Default --stage is `structural-check`, matching the most common case where
# a hand-repaired author output needs to be re-validated structurally before
# review dispatch. With --reset-status the script also flips the slice from
# escalated/blocked_retry back to in_progress before transitioning, so the
# downstream dispatch_stage call is allowed to run.

set -euo pipefail

WORKSPACE_ROOT="."
QUEUE_PATH="slices/queue.json"
ACTIVE_STATE_PATH=".mutagen/state/active-slice.json"
AUTHOR_OUTPUT_DIR=".mutagen/state/author-output"
DISPATCH_ROOT=".mutagen/state/dispatch"
SLICEMAP_PATH="slices/slicemap.md"
LEGACY_PATH="slices/queue.md"
HOST_KIND="codex"
SLICE_ID=""
STAGE="structural-check"
RESET_STATUS=0

usage() {
  cat <<'EOF' >&2
Usage:
  resume_after_escalation.sh --slice-id ID
    [--stage structural-check|review|state-record]
    [--workspace-root PATH] [--queue PATH] [--active-state PATH]
    [--author-output-dir PATH] [--dispatch-root PATH]
    [--slicemap PATH] [--legacy PATH] [--host HOST]
    [--reset-status]
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace-root) [[ $# -ge 2 ]] || usage; WORKSPACE_ROOT="$2"; shift 2;;
    --queue)          [[ $# -ge 2 ]] || usage; QUEUE_PATH="$2"; shift 2;;
    --active-state)   [[ $# -ge 2 ]] || usage; ACTIVE_STATE_PATH="$2"; shift 2;;
    --author-output-dir) [[ $# -ge 2 ]] || usage; AUTHOR_OUTPUT_DIR="$2"; shift 2;;
    --dispatch-root)  [[ $# -ge 2 ]] || usage; DISPATCH_ROOT="$2"; shift 2;;
    --slicemap)       [[ $# -ge 2 ]] || usage; SLICEMAP_PATH="$2"; shift 2;;
    --legacy)         [[ $# -ge 2 ]] || usage; LEGACY_PATH="$2"; shift 2;;
    --host)           [[ $# -ge 2 ]] || usage; HOST_KIND="$2"; shift 2;;
    --slice-id)       [[ $# -ge 2 ]] || usage; SLICE_ID="$2"; shift 2;;
    --stage)          [[ $# -ge 2 ]] || usage; STAGE="$2"; shift 2;;
    --reset-status)   RESET_STATUS=1; shift;;
    --help|-h)        usage;;
    *)                usage;;
  esac
done

[[ -n "$SLICE_ID" ]] || usage

case "$STAGE" in
  structural-check|review|state-record) ;;
  *) echo "resume_after_escalation.sh: unsupported --stage '$STAGE'" >&2; exit 1;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

resolve_jq() {
  command -v jq 2>/dev/null || command -v jq.exe 2>/dev/null
}

JQ_BIN="$(resolve_jq)" || {
  printf '{"ok":false,"error":"tooling_failure","message":"jq not found on PATH"}\n'
  exit 1
}

if [[ "$RESET_STATUS" -eq 1 ]]; then
  bash "$SCRIPT_DIR/update_queue_slice.sh" \
    --queue "$QUEUE_PATH" \
    --slicemap "$SLICEMAP_PATH" \
    --legacy "$LEGACY_PATH" \
    --slice-id "$SLICE_ID" \
    --status in_progress \
    --clear-escalation-reason \
    --clear-completed-at >/dev/null
fi

bash "$SCRIPT_DIR/transition_active_slice.sh" \
  --queue "$QUEUE_PATH" \
  --active-state "$ACTIVE_STATE_PATH" \
  --slicemap "$SLICEMAP_PATH" \
  --legacy "$LEGACY_PATH" \
  --slice-id "$SLICE_ID" \
  --stage "$STAGE" >/dev/null

if [[ "$STAGE" == "structural-check" ]]; then
  # karai_structural_check.sh takes a positional slice_id and reads queue /
  # author-output-dir from the workspace's defaults. We change into the
  # workspace before invoking it so the relative paths line up.
  STRUCTURAL_OUTPUT="$(
    cd "$WORKSPACE_ROOT" && bash "$SCRIPT_DIR/karai_structural_check.sh" "$SLICE_ID"
  )"
  STRUCTURAL_VERDICT="$(printf '%s' "$STRUCTURAL_OUTPUT" | "$JQ_BIN" -r '.verdict // .structural.verdict // ""')"

  bash "$SCRIPT_DIR/update_queue_slice.sh" \
    --queue "$QUEUE_PATH" \
    --slicemap "$SLICEMAP_PATH" \
    --legacy "$LEGACY_PATH" \
    --slice-id "$SLICE_ID" \
    --karai-structural "$STRUCTURAL_VERDICT" >/dev/null

  if [[ "$STRUCTURAL_VERDICT" != "pass" ]]; then
    "$JQ_BIN" -n \
      --arg slice_id "$SLICE_ID" \
      --argjson structural "$STRUCTURAL_OUTPUT" \
      '{
        ok: false,
        slice_id: $slice_id,
        stage: "structural-check",
        message: "structural check did not pass; investigate before re-dispatching",
        structural: $structural
      }'
    exit 1
  fi

  STAGE="review"
  bash "$SCRIPT_DIR/transition_active_slice.sh" \
    --queue "$QUEUE_PATH" \
    --active-state "$ACTIVE_STATE_PATH" \
    --slicemap "$SLICEMAP_PATH" \
    --legacy "$LEGACY_PATH" \
    --slice-id "$SLICE_ID" \
    --stage "$STAGE" >/dev/null
fi

DISPATCH_OUTPUT="$(
  bash "$SCRIPT_DIR/dispatch_stage.sh" \
    --workspace-root "$WORKSPACE_ROOT" \
    --queue "$QUEUE_PATH" \
    --active-state "$ACTIVE_STATE_PATH" \
    --author-output-dir "$AUTHOR_OUTPUT_DIR" \
    --dispatch-root "$DISPATCH_ROOT" \
    --slicemap "$SLICEMAP_PATH" \
    --legacy "$LEGACY_PATH" \
    --host "$HOST_KIND" \
    --slice-id "$SLICE_ID"
)"

"$JQ_BIN" -n \
  --arg slice_id "$SLICE_ID" \
  --arg stage "$STAGE" \
  --argjson dispatch "$DISPATCH_OUTPUT" \
  '{
    ok: true,
    slice_id: $slice_id,
    resumed_stage: $stage,
    dispatch: $dispatch
  }'

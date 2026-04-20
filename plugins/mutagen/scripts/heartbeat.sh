#!/usr/bin/env bash
# Heartbeat summary for the currently-active slice.
#
# Reads .claude/state/tool-calls/{slice_id}.jsonl (written by
# counter.sh on every PostToolUse) and emits a single-line JSON
# summary Karai can use for in-flight inspection: call count,
# calls in the last N minutes, most recent consecutive-repeat
# run length (loop signal), and rough size rate.
#
# Usage:
#   heartbeat.sh [window_seconds]
#
# Defaults to 300s (5 min) if no argument given.
#
# This is a read-only helper. Failure exits with a status-indicating
# JSON blob rather than throwing — callers can depend on stdout
# being JSON.

set -uo pipefail

WINDOW="${1:-300}"

emit() {
  printf '%s\n' "$1"
  exit 0
}

command -v jq >/dev/null 2>&1 || emit '{"ok":false,"reason":"jq missing"}'

STATE_FILE=".claude/state/active-slice.json"
[[ -f "$STATE_FILE" ]] || emit '{"ok":false,"reason":"no active slice"}'

SLICE_ID="$(jq -r '.slice_id // empty' "$STATE_FILE" 2>/dev/null || true)"
[[ -z "$SLICE_ID" ]] && emit '{"ok":false,"reason":"no slice_id in state"}'

SAFE_SLICE_ID="$(printf '%s' "$SLICE_ID" | tr -c 'A-Za-z0-9._-' '_' | cut -c1-120)"
LOG_FILE=".claude/state/tool-calls/${SAFE_SLICE_ID}.jsonl"

if [[ ! -s "$LOG_FILE" ]]; then
  emit "$(jq -cn --arg slice "$SLICE_ID" '{ok:true,slice:$slice,total:0,window_calls:0,last_run_length:0,last_run_tool:"",last_run_hash:"",bytes_last_window:0,window_seconds:'"$WINDOW"'}')"
fi

NOW_EPOCH="$(date -u +%s)"

jq -sc --arg slice "$SLICE_ID" --argjson now "$NOW_EPOCH" --argjson window "$WINDOW" '
  def to_epoch(ts): ts | sub("Z$"; "+00:00") | fromdateiso8601? // 0;

  . as $all
  | ($all | length) as $total
  | ([$all[] | select(to_epoch(.ts) >= ($now - $window))] ) as $recent
  | ($recent | length) as $window_calls
  | ($recent | map(.input_bytes // 0) | add // 0) as $bytes
  | ($all | reverse) as $rev
  | (if ($rev | length) == 0 then {tool:"",hash:"",run:0}
     else
       ($rev[0]) as $head
       | ($rev | map(select(.tool == $head.tool and .hash == $head.hash)) | length) as $run_len
       | {tool:$head.tool, hash:$head.hash, run:$run_len}
     end) as $loop
  | {
      ok: true,
      slice: $slice,
      total: $total,
      window_seconds: $window,
      window_calls: $window_calls,
      bytes_last_window: $bytes,
      last_run_tool: $loop.tool,
      last_run_hash: $loop.hash,
      last_run_length: $loop.run
    }
' "$LOG_FILE" 2>/dev/null || emit "$(jq -cn --arg slice "$SLICE_ID" '{ok:false,reason:"parse error",slice:$slice}')"

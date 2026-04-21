#!/usr/bin/env bash
# PostToolUse tool-call counter.
#
# Records every tool call into .mutagen/state/tool-calls/{slice_id}.jsonl
# whenever an active slice is in flight. Karai reads this log for
# loop detection, tokens-per-minute approximation, and heartbeat telemetry.
#
# This script MUST NEVER block a tool call. Any failure exits 0 silently.
# If there is no active slice, we do nothing — ordinary development is
# not tracked.

set -uo pipefail

PAYLOAD="$(cat || true)"

command -v jq >/dev/null 2>&1 || exit 0

CWD="$(printf '%s' "$PAYLOAD" | jq -r '.cwd // empty' 2>/dev/null || true)"
STATE_FILE="${CWD:-.}/.mutagen/state/active-slice.json"
[[ -f "$STATE_FILE" ]] || exit 0

SLICE_ID="$(jq -r '.slice_id // empty' "$STATE_FILE" 2>/dev/null || true)"
[[ -z "$SLICE_ID" ]] && exit 0

# Sanitize slice_id for filesystem use (no path traversal, no weirdness).
SAFE_SLICE_ID="$(printf '%s' "$SLICE_ID" | tr -c 'A-Za-z0-9._-' '_' | cut -c1-120)"
[[ -z "$SAFE_SLICE_ID" ]] && exit 0

STAGE="$(jq -r '.stage // empty' "$STATE_FILE" 2>/dev/null || true)"
ACTIVE_AGENT="$(jq -r '.active_agent // .author_agent // empty' "$STATE_FILE" 2>/dev/null || true)"
ATTEMPT="$(jq -r '.attempts // 0' "$STATE_FILE" 2>/dev/null || echo 0)"

TOOL="$(printf '%s' "$PAYLOAD" | jq -r '.tool_name // empty' 2>/dev/null || true)"
[[ -z "$TOOL" ]] && exit 0

INPUT="$(printf '%s' "$PAYLOAD" | jq -c '.tool_input // {}' 2>/dev/null || echo '{}')"

HASH=""
if command -v sha1sum >/dev/null 2>&1; then
  HASH="$(printf '%s' "$INPUT" | sha1sum | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  HASH="$(printf '%s' "$INPUT" | shasum -a 1 | awk '{print $1}')"
fi

# Rough size proxy for TPM approximation — not a true token count, but
# stable enough for "is this agent gushing or stalled?" decisions.
INPUT_BYTES=${#INPUT}

# Did the tool error out? Claude Code includes `tool_response` on
# PostToolUse; surface a boolean for Karai's log parsing.
IS_ERROR="$(printf '%s' "$PAYLOAD" | jq -r '(.tool_response.is_error // false) | tostring' 2>/dev/null || echo false)"

TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

LOG_DIR="${CWD:-.}/.mutagen/state/tool-calls"
mkdir -p "$LOG_DIR" 2>/dev/null || exit 0
LOG_FILE="$LOG_DIR/${SAFE_SLICE_ID}.jsonl"

jq -cn \
  --arg ts "$TS" \
  --arg slice "$SLICE_ID" \
  --arg stage "$STAGE" \
  --arg agent "$ACTIVE_AGENT" \
  --arg tool "$TOOL" \
  --arg hash "$HASH" \
  --argjson bytes "$INPUT_BYTES" \
  --argjson attempt "$ATTEMPT" \
  --argjson is_error "$IS_ERROR" \
  '{ts:$ts, slice:$slice, stage:$stage, agent:$agent, tool:$tool, hash:$hash, input_bytes:$bytes, attempt:$attempt, is_error:$is_error}' \
  >> "$LOG_FILE" 2>/dev/null || true

exit 0

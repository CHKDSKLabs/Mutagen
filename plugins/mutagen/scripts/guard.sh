#!/usr/bin/env bash
# PreToolUse guard for Write / Edit.
#
# Contract:
#   - Reads Claude Code's PreToolUse JSON from stdin.
#   - Exits 0 to allow the Write / Edit, exits 2 to block.
#   - On block, writes a reason to stderr that Claude will see.
#
# Logic (in order):
#   1. If the tool isn't Write or Edit, allow.
#   2. Universal denylist: no one writes to the design bundle
#      (templates/, guides/, upstream design docs under docs/ or
#      repo-root PRD/ADR/DDD/ISC/DSD variants) unless an active
#      slice explicitly allows that path. The lone exception is
#      April: when `.claude/state/active-slice.json` is present
#      with `"author_agent": "April"`, the bundle may be edited.
#   3. If an active-slice state file is present, enforce its
#      `allowed_write_globs`. A write outside that list is blocked.
#   4. If no active-slice state file exists, allow (the session is
#      not executing a slice; ordinary development).
#
# Dependencies: bash, jq. If jq is missing the guard fails open
# (exit 0) with a warning on stderr so the harness is never
# bricked by a missing dependency — set `STRICT_GUARD=1` in the
# environment to fail closed instead.

set -euo pipefail

PAYLOAD="$(cat)"

have_jq() { command -v jq >/dev/null 2>&1; }

if ! have_jq; then
  if [[ "${STRICT_GUARD:-0}" == "1" ]]; then
    echo "guard.sh: jq not found and STRICT_GUARD=1; blocking." >&2
    exit 2
  fi
  echo "guard.sh: jq not found; allowing write (install jq for scope enforcement)." >&2
  exit 0
fi

TOOL_NAME="$(printf '%s' "$PAYLOAD" | jq -r '.tool_name // empty')"
case "$TOOL_NAME" in
  Write|Edit) ;;
  *) exit 0 ;;
esac

FILE_PATH="$(printf '%s' "$PAYLOAD" | jq -r '.tool_input.file_path // empty')"
if [[ -z "$FILE_PATH" ]]; then
  exit 0
fi

CWD="$(printf '%s' "$PAYLOAD" | jq -r '.cwd // empty')"
# Windows hands us C:\foo\bar paths; globs below are forward-slash. Flatten
# both before the prefix strip or every match silently fails and the slice
# can't write to its own allowlist.
REL_PATH="${FILE_PATH//\\//}"
CWD_NORM="${CWD//\\//}"
if [[ -n "$CWD_NORM" && "$REL_PATH" == "$CWD_NORM"/* ]]; then
  REL_PATH="${REL_PATH#"$CWD_NORM"/}"
fi

match_glob() {
  local path="$1"
  shift
  local glob
  for glob in "$@"; do
    # shellcheck disable=SC2053
    if [[ "$path" == $glob ]]; then
      return 0
    fi
  done
  return 1
}

STATE_FILE="${CWD:-.}/.claude/state/active-slice.json"

ACTIVE_AGENT=""
declare -a ALLOWED=()
if [[ -f "$STATE_FILE" ]]; then
  # WinGet's jq 1.8.1 helpfully CRLFs its stdout regardless of input line endings,
  # so every glob ends up stored as "glob\r" and matches exactly nothing. Strip it.
  ACTIVE_AGENT="$(jq -r '.author_agent // empty' "$STATE_FILE" 2>/dev/null | tr -d '\r' || true)"
  while IFS= read -r glob; do
    glob="${glob%$'\r'}"
    [[ -n "$glob" ]] && ALLOWED+=("$glob")
  done < <(jq -r '.allowed_write_globs[]? // empty' "$STATE_FILE" 2>/dev/null || true)
fi

# Universal denylist: design bundle.
# Exception: April (design-phase elicitor) may edit the bundle when
# an active slice identifies her as the author.
BUNDLE_GLOBS=(
  'templates/**'
  'guides/**'
  'docs/PRD*'
  'docs/PRD/**'
  'docs/ADR*'
  'docs/ADR/**'
  'docs/DDD*'
  'docs/DDD/**'
  'docs/ISC*'
  'docs/ISC/**'
  'docs/DSD*'
  'docs/DSD/**'
  'PRD*.md'
  'ADR*.md'
  'DDD*.md'
  'ISC*.md'
  'DSD*.md'
  'design/**'
)

if match_glob "$REL_PATH" "${BUNDLE_GLOBS[@]}"; then
  # Allow April to author or edit the bundle.
  if [[ "$ACTIVE_AGENT" == "April" ]]; then
    # April still needs to stay within her allowed_write_globs,
    # which will be checked below.
    :
  else
    # Within-plugin meta editing — e.g. working on this repo itself —
    # must be an explicit override. Set CLAUDE_WORKFLOW_META=1 to
    # allow plugin-internal edits to templates/ or guides/.
    if [[ "${CLAUDE_WORKFLOW_META:-0}" != "1" ]]; then
      {
        echo "guard.sh: Write to upstream design bundle blocked."
        echo "  path: $REL_PATH"
        echo "  reason: design scaffolds and the instantiated upstream bundle"
        echo "          (PRD / ADR / DDD / ISC / DSD, templates/, guides/)"
        echo "          are owned by April. Run /mutagen:elicit or set"
        echo "          CLAUDE_WORKFLOW_META=1 for plugin-internal edits."
      } >&2
      exit 2
    fi
  fi
fi

# Active-slice enforcement: if a slice is in flight, writes must
# match its allowlist. Outside a slice, allow.
if [[ ${#ALLOWED[@]} -gt 0 ]]; then
  if ! match_glob "$REL_PATH" "${ALLOWED[@]}"; then
    {
      echo "guard.sh: Write outside active slice scope blocked."
      echo "  path:  $REL_PATH"
      echo "  slice: $(jq -r '.slice_id // "<unknown>"' "$STATE_FILE" 2>/dev/null)"
      echo "  agent: ${ACTIVE_AGENT:-<unknown>}"
      echo "  allowed globs:"
      printf '    - %s\n' "${ALLOWED[@]}"
      echo "  Edit .claude/state/active-slice.json to extend the allowlist,"
      echo "  or halt the slice and escalate to the user."
    } >&2
    exit 2
  fi
fi

exit 0

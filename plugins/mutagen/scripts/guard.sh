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
#      April: when `.mutagen/state/active-slice.json` is present
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

filesystem_path() {
  local path="${1:-}"

  if [[ -z "$path" ]]; then
    printf '.'
    return 0
  fi

  if [[ "$path" =~ ^([A-Za-z]):[\\/](.*)$ ]]; then
    local drive="${BASH_REMATCH[1],,}"
    local rest="${BASH_REMATCH[2]//\\//}"
    printf '/mnt/%s/%s' "$drive" "$rest"
    return 0
  fi

  printf '%s' "$path"
}

FS_CWD="$(filesystem_path "$CWD")"
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

first_matching_glob() {
  local path="$1"
  shift
  local glob
  for glob in "$@"; do
    # shellcheck disable=SC2053
    if [[ "$path" == $glob ]]; then
      printf '%s' "$glob"
      return 0
    fi
  done
  return 1
}

STATE_FILE="${FS_CWD:-.}/.mutagen/state/active-slice.json"

ACTIVE_AGENT=""
AUTHOR_AGENT=""
SLICE_ID=""
SLICE_TITLE=""
STAGE=""
declare -a ALLOWED=()
if [[ -f "$STATE_FILE" ]]; then
  # WinGet's jq 1.8.1 helpfully CRLFs its stdout regardless of input line endings,
  # so every glob ends up stored as "glob\r" and matches exactly nothing. Strip it.
  ACTIVE_AGENT="$(jq -r '.author_agent // empty' "$STATE_FILE" 2>/dev/null | tr -d '\r' || true)"
  AUTHOR_AGENT="$(jq -r '.author_agent // empty' "$STATE_FILE" 2>/dev/null | tr -d '\r' || true)"
  ACTIVE_AGENT="$(jq -r '.active_agent // .author_agent // empty' "$STATE_FILE" 2>/dev/null | tr -d '\r' || true)"
  SLICE_ID="$(jq -r '.slice_id // empty' "$STATE_FILE" 2>/dev/null | tr -d '\r' || true)"
  SLICE_TITLE="$(jq -r '.title // empty' "$STATE_FILE" 2>/dev/null | tr -d '\r' || true)"
  STAGE="$(jq -r '.stage // empty' "$STATE_FILE" 2>/dev/null | tr -d '\r' || true)"
  while IFS= read -r glob; do
    glob="${glob%$'\r'}"
    [[ -n "$glob" ]] && ALLOWED+=("$glob")
  done < <(jq -r '.allowed_write_globs[]? // empty' "$STATE_FILE" 2>/dev/null || true)
fi

persist_scope_violation() {
  local class="$1"
  local matched_rule="${2:-}"
  local reason="$3"
  local state_dir
  local allowed_json
  local ts
  local body

  state_dir="${FS_CWD:-.}/.mutagen/state"
  mkdir -p "$state_dir" >/dev/null 2>&1 || return 0

  if [[ ${#ALLOWED[@]} -gt 0 ]]; then
    allowed_json="$(printf '%s\n' "${ALLOWED[@]}" | jq -R . | jq -s . 2>/dev/null || printf '[]')"
  else
    allowed_json='[]'
  fi

  ts="$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || printf '')"
  body="$(
    jq -nc \
      --argjson allowed_write_globs "$allowed_json" \
      --arg ts "$ts" \
      --arg decision "deny" \
      --arg class "$class" \
      --arg matched_rule "$matched_rule" \
      --arg tool_name "$TOOL_NAME" \
      --arg path "$REL_PATH" \
      --arg reason "$reason" \
      --arg message "Traag DENY on $REL_PATH (class: $class) during stage ${STAGE:-unknown}. Agent: ${ACTIVE_AGENT:-unknown}." \
      --arg slice_id "$SLICE_ID" \
      --arg title "$SLICE_TITLE" \
      --arg stage "$STAGE" \
      --arg active_agent "$ACTIVE_AGENT" \
      --arg author_agent "$AUTHOR_AGENT" \
      '{
        ts: (if $ts == "" then null else $ts end),
        decision: $decision,
        class: $class,
        matched_rule: (if $matched_rule == "" then null else $matched_rule end),
        tool_name: (if $tool_name == "" then null else $tool_name end),
        path: $path,
        reason: $reason,
        message: $message,
        slice_id: (if $slice_id == "" then null else $slice_id end),
        title: (if $title == "" then null else $title end),
        stage: (if $stage == "" then null else $stage end),
        active_agent: (if $active_agent == "" then null else $active_agent end),
        author_agent: (if $author_agent == "" then null else $author_agent end),
        allowed_write_globs: $allowed_write_globs
      }' 2>/dev/null
  )"

  [[ -z "$body" ]] && return 0

  printf '%s\n' "$body" > "$state_dir/scope-violation.json" 2>/dev/null || true
  printf '%s\n' "$body" >> "$state_dir/scope-violations.jsonl" 2>/dev/null || true
}

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
      matched_rule="$(first_matching_glob "$REL_PATH" "${BUNDLE_GLOBS[@]}" || true)"
      persist_scope_violation "global" "$matched_rule" "write to upstream design bundle blocked"
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
    persist_scope_violation "out_of_scope" "" "write outside active slice scope blocked"
    {
      echo "guard.sh: Write outside active slice scope blocked."
      echo "  path:  $REL_PATH"
      echo "  slice: $(jq -r '.slice_id // "<unknown>"' "$STATE_FILE" 2>/dev/null)"
      echo "  agent: ${ACTIVE_AGENT:-<unknown>}"
      echo "  allowed globs:"
      printf '    - %s\n' "${ALLOWED[@]}"
      echo "  Edit .mutagen/state/active-slice.json to extend the allowlist,"
      echo "  or halt the slice and escalate to the user."
    } >&2
    exit 2
  fi
fi

exit 0

#!/usr/bin/env bash
# Structural conformance check for a completed author dispatch.
#
# Replaces Karai's Stage 2 agent invocation. Section-presence / identifier /
# trace-ID / state-block / LOC checks are pattern-matching work, not judgment
# work — a script handles them without spawning an agent per slice per attempt.
#
# Usage:
#   karai_structural_check.sh <slice_id>
#
# Reads:
#   - slices/queue.json                              — slice metadata (traces_to, context_to_update, author_agent, target_loc)
#   - .mutagen/state/author-output/<slice_id>.md     — the author's most recent return, written by execute-next.md Stage 1 before dispatching this check
#   - project_state.md / infrastructure_state.md     — to verify the State Update block landed
#   - scripts/slice_loc.sh                           — called for LOC telemetry
#
# Emits (stdout, single JSON object):
#   {
#     "verdict": "pass" | "fail",
#     "findings": [
#       {"check": "<name>", "severity": "fail" | "warn", "detail": "<one-line>"}
#     ],
#     "loc": { ... passthrough of slice_loc.sh output ... }
#   }
#
# The orchestrator branches on `.verdict`: "pass" → continue to Stage 3; "fail"
# → escalate with findings verbatim. Karai the agent is no longer dispatched at
# Stage 2; she only runs Stage 4 (state verify + dispatch log + advisory backlog)
# and review escalations.

set -u

slice_id="${1:-}"
if [ -z "$slice_id" ]; then
  echo '{"verdict":"fail","findings":[{"check":"args","severity":"fail","detail":"missing slice_id"}]}'
  exit 0
fi

if ! command -v jq >/dev/null 2>&1; then
  echo '{"verdict":"fail","findings":[{"check":"tooling","severity":"fail","detail":"jq not installed"}]}'
  exit 0
fi

queue="slices/queue.json"
if [ ! -r "$queue" ]; then
  echo '{"verdict":"fail","findings":[{"check":"queue","severity":"fail","detail":"slices/queue.json not readable"}]}'
  exit 0
fi

slice_json=$(jq -c --arg id "$slice_id" '.slices[] | select(.id == $id)' "$queue" 2>/dev/null)
if [ -z "$slice_json" ] || [ "$slice_json" = "null" ]; then
  printf '{"verdict":"fail","findings":[{"check":"queue","severity":"fail","detail":"slice %s not found"}]}\n' "$slice_id"
  exit 0
fi

author_agent=$(printf '%s' "$slice_json" | jq -r '.author_agent // ""')
context_file=$(printf '%s' "$slice_json" | jq -r '.context_to_update // ""')
target_loc=$(printf '%s' "$slice_json" | jq -r '.target_loc // 0')

# Required-section patterns per author. The emoji header anchors the output
# block; other headers are matched as markdown subsections. Regex is run as
# fixed-string grep against the captured author output.
required_sections=""
case "$author_agent" in
  Bebop)
    required_sections=$'🛠️ Execution:\nIntake Report\nCode Artifacts\nISC Upholding Map\nVerification Artifacts\nState Update'
    ;;
  Baxter)
    required_sections=$'🔬 Execution:\nIntake Report\nAlgorithmic Proof\nCode Artifacts\nISC Upholding Map\nVerification Artifacts\nState Update'
    ;;
  Chaplin)
    required_sections=$'💽 Execution:\nIntake Report\nData Model Analysis\nCode Artifacts\nISC Upholding Map\nVerification Artifacts\nState Update'
    ;;
  Metalhead)
    required_sections=$'📡 Execution:\nIntake Report\nObservability Plan\nCode Artifacts\nISC Upholding Map\nVerification Artifacts\nState Update'
    ;;
  Splinter)
    required_sections=$'🐀 Execution:\nIntake Report\nDocumentation Brief\nDrafted Artefacts\nCross-check Notes\nVerification Artifacts\nState Update'
    ;;
  Tatsu)
    required_sections=$'🥷 Execution:\nIntake Report\nThreat Model\nCode Artifacts\nISC Upholding Map\nVerification Artifacts\nState Update'
    ;;
  Krang)
    required_sections=$'🧠 Execution:\nIntake Report\nInfrastructure Artifacts\nISC Enforcement Map\nVerification Artifacts\nState Update'
    ;;
  *)
    printf '{"verdict":"fail","findings":[{"check":"author_agent","severity":"fail","detail":"unknown author_agent %s"}]}\n' "$author_agent"
    exit 0
    ;;
esac

author_output=".mutagen/state/author-output/$slice_id.md"
findings_arr="[]"

append_finding() {
  local check="$1" severity="$2" detail="$3"
  findings_arr=$(printf '%s' "$findings_arr" | jq -c --arg c "$check" --arg s "$severity" --arg d "$detail" \
    '. + [{"check":$c,"severity":$s,"detail":$d}]')
}

if [ ! -r "$author_output" ]; then
  append_finding "author_output" "fail" "author output not found at $author_output — orchestrator must write it before calling this script"
  printf '{"verdict":"fail","findings":%s}\n' "$findings_arr"
  exit 0
fi

# 1. Required sections present.
while IFS= read -r section; do
  [ -z "$section" ] && continue
  if ! grep -qF "$section" "$author_output"; then
    append_finding "required_section" "fail" "missing required section: $section"
  fi
done <<< "$required_sections"

# 2. Traces-to drift. Every cited ID in the slice must appear somewhere in the
#    author's output (either the ISC/Enforcement Map, the Intake Report's
#    Traces-to echo, or the verification artifacts).
cited_ids=$(printf '%s' "$slice_json" | jq -r '
  [
    (.traces_to.prd // []) | .[],
    (.traces_to.adr // []) | .[],
    (.traces_to.isc // []) | .[],
    (.traces_to.dsd // []) | .[]
  ] | .[]
' 2>/dev/null)

while IFS= read -r cid; do
  [ -z "$cid" ] && continue
  if ! grep -qF "$cid" "$author_output"; then
    append_finding "traces_to_drift" "fail" "cited ID $cid does not appear in author output"
  fi
done <<< "$cited_ids"

# 3. State block landed in the correct context file.
if [ -n "$context_file" ]; then
  if [ ! -r "$context_file" ]; then
    append_finding "state_block" "fail" "context_to_update file $context_file does not exist"
  else
    if ! grep -qF "$slice_id" "$context_file"; then
      append_finding "state_block" "fail" "slice_id $slice_id not found in $context_file — State Update block likely not appended"
    fi
  fi
fi

# 4. LOC vs target. Delegate to slice_loc.sh; the script handles git + path
#    filtering. We treat > 120% of target as a hard fail.
loc_json='{}'
if [ -x "$(dirname "$0")/slice_loc.sh" ] || [ -r "$(dirname "$0")/slice_loc.sh" ]; then
  loc_json=$(bash "$(dirname "$0")/slice_loc.sh" "$slice_id" 2>/dev/null || echo '{}')
fi
over_pct=$(printf '%s' "$loc_json" | jq -r '.over_target_pct // 0' 2>/dev/null || echo 0)
if [ "${over_pct:-0}" -gt 120 ]; then
  append_finding "loc_overrun" "fail" "net LOC is ${over_pct}% of target ${target_loc} — exceeds 120% hard gate"
fi

# Aggregate verdict.
fail_count=$(printf '%s' "$findings_arr" | jq '[.[] | select(.severity == "fail")] | length')
verdict="pass"
if [ "$fail_count" -gt 0 ]; then
  verdict="fail"
fi

jq -n --arg v "$verdict" --argjson f "$findings_arr" --argjson l "$loc_json" \
  '{verdict:$v, findings:$f, loc:$l}'

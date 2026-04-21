#!/usr/bin/env bash
# Spawn a single mutagen agent via `codex exec --profile <name>`.
# Persona text (plugins/mutagen/agents/<Name>.md) is prepended to the task
# prompt so the profile is stateless re: identity — swap models freely.
#
# Usage:
#   agent.sh <PersonaName> "<task prompt>"
#   agent.sh Shredder "Slice the bundle at docs/. Pipeline mode: full."
#
# Env:
#   MUTAGEN_ROOT   path to plugins/mutagen/ (required)
#   CODEX_BIN      codex executable (default: codex)

set -euo pipefail

persona="${1:-}"
prompt="${2:-}"

if [[ -z "$persona" || -z "$prompt" ]]; then
  echo "usage: agent.sh <PersonaName> \"<prompt>\"" >&2
  exit 2
fi

: "${MUTAGEN_ROOT:?MUTAGEN_ROOT not set — re-run installer or export it manually}"

persona_file="${MUTAGEN_ROOT}/agents/${persona}.md"
if [[ ! -f "$persona_file" ]]; then
  echo "agent.sh: no persona file at $persona_file" >&2
  exit 3
fi

profile="$(echo "$persona" | tr '[:upper:]' '[:lower:]')"

codex="${CODEX_BIN:-codex}"

# Strip Claude-only frontmatter (tools:, model:) — it means nothing to Codex
# and clutters the framing block. The description + body are all we need.
persona_body="$(awk '
  /^---[[:space:]]*$/ { in_fm = !in_fm; next }
  !in_fm { print }
' "$persona_file")"

read -r -d '' framing <<EOF || true
# You are ${persona}

$persona_body

---

# Current task

$prompt
EOF

exec "$codex" exec \
  --profile "$profile" \
  --skip-git-repo-check \
  "$framing"

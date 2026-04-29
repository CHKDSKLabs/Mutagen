#!/usr/bin/env bash
# Spawn a single mutagen agent through the selected host launcher.
# Persona text (plugins/mutagen/agents/<Name>.md) is prepended to the task
# prompt so host profiles can stay stateless re: identity.
#
# Usage:
#   agent.sh [--host codex|claude] <PersonaName> "<task prompt>"
#   agent.sh --host codex Shredder "Slice the bundle at docs/. Pipeline mode: full."
#
# Env:
#   MUTAGEN_ROOT   path to plugins/mutagen/ (required)
#   MUTAGEN_AGENT_LAUNCHER
#                  optional executable override; receives:
#                  <host> <persona> <profile> <framing>
#   CODEX_BIN      codex executable (default: codex)
#   CLAUDE_BIN     claude executable (default: claude)

set -euo pipefail

host="${MUTAGEN_HOST:-codex}"

usage() {
  echo "usage: agent.sh [--host HOST] <PersonaName> \"<prompt>\"" >&2
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --host)
      [[ $# -ge 2 ]] || usage
      host="$2"
      shift 2
      ;;
    --help|-h)
      usage
      ;;
    *)
      break
      ;;
  esac
done

persona="${1:-}"
prompt="${2:-}"

if [[ -z "$persona" || -z "$prompt" ]]; then
  usage
fi

: "${MUTAGEN_ROOT:?MUTAGEN_ROOT not set — re-run installer or export it manually}"

persona_file="${MUTAGEN_ROOT}/agents/${persona}.md"
if [[ ! -f "$persona_file" ]]; then
  echo "agent.sh: no persona file at $persona_file" >&2
  exit 3
fi

profile="$(echo "$persona" | tr '[:upper:]' '[:lower:]')"

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

if [[ -n "${MUTAGEN_AGENT_LAUNCHER:-}" ]]; then
  exec "$MUTAGEN_AGENT_LAUNCHER" "$host" "$persona" "$profile" "$framing"
fi

case "$host" in
  codex)
    codex="${CODEX_BIN:-codex}"
    exec "$codex" exec \
      --profile "$profile" \
      --skip-git-repo-check \
      "$framing"
    ;;
  claude)
    claude="${CLAUDE_BIN:-claude}"
    exec "$claude" --print "$framing"
    ;;
  *)
    echo "agent.sh: unsupported host '$host'. Set MUTAGEN_AGENT_LAUNCHER to provide a custom launcher." >&2
    exit 4
    ;;
esac

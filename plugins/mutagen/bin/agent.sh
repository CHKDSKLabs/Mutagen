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

# Strip ONLY the first YAML frontmatter block: `---` on line 1 opens, the next
# `---` closes. Any later `---` in the body is a normal Markdown horizontal
# rule and must be preserved. The previous toggle-on-every-`---` behavior
# corrupted personas that used `---` as a section separator.
persona_body="$(awk '
  NR == 1 && /^---[[:space:]]*$/ {
    in_fm = 1
    next
  }
  in_fm && /^---[[:space:]]*$/ {
    in_fm = 0
    next
  }
  in_fm { next }
  { print }
' "$persona_file")"

read -r -d '' framing <<EOF || true
# You are ${persona}

$persona_body

---

# Current task

$prompt
EOF

if [[ -n "${MUTAGEN_AGENT_LAUNCHER:-}" ]]; then
  # Launcher contract (see issues/solved/CodexPro.md): a custom launcher
  # inherits this script's framing verbatim and should not preserve stdin
  # unless its target host profile explicitly asks for it. Mutagen does not
  # feed agent input over stdin. To opt back in, the launcher author can set
  # MUTAGEN_AGENT_LAUNCHER_KEEP_STDIN=1 before invoking agent.sh.
  if [[ "${MUTAGEN_AGENT_LAUNCHER_KEEP_STDIN:-0}" != "1" ]]; then
    exec "$MUTAGEN_AGENT_LAUNCHER" "$host" "$persona" "$profile" "$framing" </dev/null
  else
    exec "$MUTAGEN_AGENT_LAUNCHER" "$host" "$persona" "$profile" "$framing"
  fi
fi

case "$host" in
  codex)
    codex="${CODEX_BIN:-codex}"
    # codex exec on a non-TTY shell will block on "Reading additional input
    # from stdin..." if it inherits a live stdin. Mutagen never feeds the
    # author personas through stdin — the framing carries every byte the
    # agent needs — so we close stdin explicitly. See issues/solved/CodexPro.md
    # for the matching contract spec and the regression test that proves it.
    exec "$codex" exec \
      --profile "$profile" \
      --skip-git-repo-check \
      "$framing" \
      </dev/null
    ;;
  claude)
    # Default to the packaged non-interactive wrapper (claude --print
    # --permission-mode bypassPermissions). When CLAUDE_BIN points at the bare
    # `claude` binary we still need to pass --print so the subprocess does not
    # fall into an interactive REPL and stall the harness.
    claude="${CLAUDE_BIN:-${MUTAGEN_ROOT}/bin/claude-harness.sh}"
    if [[ "$(basename "$claude")" == "claude-harness.sh" ]]; then
      exec "$claude" "$framing"
    else
      exec "$claude" --print --permission-mode bypassPermissions "$framing"
    fi
    ;;
  *)
    echo "agent.sh: unsupported host '$host'. Set MUTAGEN_AGENT_LAUNCHER to provide a custom launcher." >&2
    exit 4
    ;;
esac

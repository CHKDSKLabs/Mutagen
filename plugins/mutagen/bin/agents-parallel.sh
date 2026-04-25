#!/usr/bin/env bash
# Spawn two mutagen agents concurrently and wait for both. Output of each goes
# to .mutagen/state/<persona>.stdout so the orchestrator can read them back
# without stdout interleaving.
#
# Usage:
#   agents-parallel.sh [--host HOST] <PersonaA> <PersonaB> "<shared prompt>"
#   agents-parallel.sh --host claude Bishop TigerClaw "$(cat prompt.md)"

set -euo pipefail

host="${MUTAGEN_HOST:-codex}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --host)
      [[ $# -ge 2 ]] || {
        echo "usage: agents-parallel.sh [--host HOST] <PersonaA> <PersonaB> \"<shared prompt>\"" >&2
        exit 2
      }
      host="$2"
      shift 2
      ;;
    --help|-h)
      echo "usage: agents-parallel.sh [--host HOST] <PersonaA> <PersonaB> \"<shared prompt>\"" >&2
      exit 2
      ;;
    *)
      break
      ;;
  esac
done

a="${1:?missing persona A}"
b="${2:?missing persona B}"
prompt="${3:?missing prompt}"

: "${MUTAGEN_ROOT:?MUTAGEN_ROOT not set}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
state_dir="$(pwd)/.mutagen/state"
mkdir -p "$state_dir"

out_a="$state_dir/$(echo "$a" | tr '[:upper:]' '[:lower:]').stdout"
out_b="$state_dir/$(echo "$b" | tr '[:upper:]' '[:lower:]').stdout"

"$here/agent.sh" --host "$host" "$a" "$prompt" >"$out_a" 2>&1 &
pid_a=$!
"$here/agent.sh" --host "$host" "$b" "$prompt" >"$out_b" 2>&1 &
pid_b=$!

fail=0
wait "$pid_a" || fail=$?
wait "$pid_b" || fail=$?

echo "--- $a ($out_a) ---"
cat "$out_a"
echo
echo "--- $b ($out_b) ---"
cat "$out_b"

exit "$fail"

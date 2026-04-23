#!/usr/bin/env bash

set -euo pipefail

resolve_jq() {
  if command -v jq >/dev/null 2>&1; then
    command -v jq
    return 0
  fi

  if command -v jq.exe >/dev/null 2>&1; then
    command -v jq.exe
    return 0
  fi

  return 1
}

JQ_BIN="$(resolve_jq)" || exit 0
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PAYLOAD="$(cat)"

if [[ -z "$PAYLOAD" ]]; then
  exit 0
fi

while IFS= read -r notification; do
  [[ -z "$notification" ]] && continue

  event="$(printf '%s' "$notification" | "$JQ_BIN" -r '.event // empty')"
  title="$(printf '%s' "$notification" | "$JQ_BIN" -r '.title // "mutagen"')"
  message="$(printf '%s' "$notification" | "$JQ_BIN" -r '.message // "(no message)"')"

  [[ -z "$event" ]] && continue

  "$SCRIPT_DIR/notify.sh" "$event" "$title" "$message" >/dev/null 2>&1 || true
done < <(printf '%s' "$PAYLOAD" | "$JQ_BIN" -c '(.notifications // [])[]?')

exit 0

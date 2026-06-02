#!/usr/bin/env bash

# SessionStart UX nicety: if the packaged harness binary is missing, kick off
# the fetch script in the background so the user's first real harness call
# doesn't pay the download latency. Failures are swallowed by design — the
# runtime path in harness_runtime.sh is the source of truth.

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Already-set override: nothing to do.
if [[ -n "${MUTAGEN_HARNESS_BIN:-}" ]]; then
  exit 0
fi

# Already packaged locally: nothing to do.
if [[ -x "$PLUGIN_ROOT/bin/mutagen-harness" || -x "$PLUGIN_ROOT/bin/mutagen-harness.exe" ]]; then
  exit 0
fi

# Operator opt-out.
if [[ "${MUTAGEN_NO_AUTOFETCH:-0}" -eq 1 ]]; then
  exit 0
fi

FETCH="$PLUGIN_ROOT/scripts/fetch_harness_binary.sh"
[[ -f "$FETCH" ]] || exit 0

LOG_DIR="$PLUGIN_ROOT/bin"
mkdir -p "$LOG_DIR" 2>/dev/null || true
LOG="$LOG_DIR/.harness-fetch.log"

# Detached, non-blocking. setsid where available so a slow GitHub doesn't keep
# the session start spinning. nohup as the portable fallback.
if command -v setsid >/dev/null 2>&1; then
  setsid bash "$FETCH" --quiet >"$LOG" 2>&1 < /dev/null &
elif command -v nohup >/dev/null 2>&1; then
  nohup bash "$FETCH" --quiet >"$LOG" 2>&1 < /dev/null &
else
  bash "$FETCH" --quiet >"$LOG" 2>&1 < /dev/null &
fi

disown 2>/dev/null || true
exit 0

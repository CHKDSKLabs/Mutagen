#!/usr/bin/env bash
# Manage the harness pause sentinel.
#
# `pause` is a stage-boundary contract: when the sentinel file exists, the
# execute-next runner stops at the next stage boundary (between slices, after
# a finalize, or before claiming a new slice) instead of starting another
# loop iteration. It does NOT pre-empt work that is already in flight inside
# a Rust dispatch — for that, kill the offending process directly.
#
# Usage:
#   pause.sh on   [--reason TEXT]
#   pause.sh off
#   pause.sh status
#
# Defaults to `status` when no subcommand is given. The sentinel lives at
# .mutagen/state/pause.json (workspace-relative). Override the workspace via
# MUTAGEN_WORKSPACE_ROOT if you are calling from outside the project.

set -euo pipefail

WORKSPACE_ROOT="${MUTAGEN_WORKSPACE_ROOT:-$(pwd)}"
SENTINEL_PATH="$WORKSPACE_ROOT/.mutagen/state/pause.json"

usage() {
  cat <<'EOF' >&2
Usage:
  pause.sh on   [--reason TEXT]
  pause.sh off
  pause.sh status
EOF
  exit 1
}

subcommand="${1:-status}"
shift || true

reason=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --reason) [[ $# -ge 2 ]] || usage; reason="$2"; shift 2;;
    --help|-h) usage;;
    *) usage;;
  esac
done

case "$subcommand" in
  on)
    mkdir -p "$(dirname "$SENTINEL_PATH")"
    timestamp="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
    if command -v jq >/dev/null 2>&1; then
      jq -n \
        --arg paused_at "$timestamp" \
        --arg reason "$reason" \
        '{paused_at: $paused_at, reason: $reason}' \
        > "$SENTINEL_PATH"
    else
      printf '{"paused_at":"%s","reason":"%s"}\n' "$timestamp" "$reason" \
        > "$SENTINEL_PATH"
    fi
    printf '{"ok":true,"state":"paused","sentinel":"%s"}\n' "$SENTINEL_PATH"
    ;;
  off)
    if [[ -f "$SENTINEL_PATH" ]]; then
      rm -f "$SENTINEL_PATH"
      printf '{"ok":true,"state":"running","sentinel":"%s","cleared":true}\n' "$SENTINEL_PATH"
    else
      printf '{"ok":true,"state":"running","sentinel":"%s","cleared":false}\n' "$SENTINEL_PATH"
    fi
    ;;
  status)
    if [[ -f "$SENTINEL_PATH" ]]; then
      if command -v jq >/dev/null 2>&1; then
        jq --arg sentinel "$SENTINEL_PATH" \
          '{ok: true, state: "paused", sentinel: $sentinel} + .' \
          "$SENTINEL_PATH"
      else
        printf '{"ok":true,"state":"paused","sentinel":"%s"}\n' "$SENTINEL_PATH"
      fi
    else
      printf '{"ok":true,"state":"running","sentinel":"%s"}\n' "$SENTINEL_PATH"
    fi
    ;;
  *)
    usage
    ;;
esac
